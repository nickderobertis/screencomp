//! Optional `screencomp.toml` configuration for the `comment` and `scope`
//! commands.
//!
//! Every field has a default, so the tool runs without any config file.
//! Resolution precedence: an explicit `--config`, then the [`CONFIG_ENV`]
//! environment variable, then an auto-discovered [`CONFIG_FILE`] found by walking
//! up from the working directory, then built-in defaults. An *explicit* source
//! (`--config`/env) that names a missing file is a hard error, so a typo surfaces
//! instead of silently falling back; an absent *auto-discovered* file simply
//! yields defaults. A file that is found but invalid is always an error.

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

/// Environment variable consulted for a config path when `--config` is absent.
pub(crate) const CONFIG_ENV: &str = "SCREENCOMP_CONFIG";

/// Config filename auto-discovered by walking up from the working directory.
pub(crate) const CONFIG_FILE: &str = "screencomp.toml";

/// Validated configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    /// Capture-wide settings (the supported CPU architectures).
    pub(crate) capture: CaptureConfig,
    /// Settings for the rendered pull-request comment.
    pub(crate) comment: CommentConfig,
    /// Settings for the optional local pre-push guard.
    pub(crate) guard: GuardConfig,
}

/// Capture-wide settings.
///
/// The single source of truth for which CPU architectures the project maintains
/// screenshots for. Local commands default to the host arch and require it to be
/// listed here (each arch has its own committed baseline); CI fans out one
/// capture lane per entry. Empty (the default) means no arch layer at all — the
/// root is treated as project-level, for ad-hoc use without a config file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CaptureConfig {
    /// CPU architectures with committed baselines and CI lanes, e.g. `["arm64"]`.
    pub(crate) arches: Vec<String>,
}

/// Comment-rendering settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommentConfig {
    /// Heading shown at the top of the comment.
    pub(crate) title: String,
    /// Stable identifier embedded as an HTML marker so the comment can be upserted.
    pub(crate) marker: String,
    /// Whether to list unchanged screenshots in the comment.
    pub(crate) show_unchanged: bool,
    /// Embed inline image previews when at most this many screenshots differ
    /// (added + changed + removed) and a gallery URL is available.
    pub(crate) embed_limit: usize,
}

/// Settings for the optional local pre-push guard (see `examples/pre-push`).
///
/// The guard re-captures and re-classifies only when screenshot-relevant files
/// change. These fields describe what counts as relevant and where the
/// committed baseline and review gallery live; every field is optional so a
/// repository that does not use the guard need not configure it. Only
/// [`paths`](Self::paths) is consumed by `screencomp scope`; the rest are read
/// by the hook template to keep its capture/classify wiring in one place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct GuardConfig {
    /// Globs whose match against a pushed change set triggers a re-capture.
    /// Empty (the default) means the guard never fires.
    pub(crate) paths: Vec<String>,
    /// Committed digest manifest used as the baseline.
    pub(crate) manifest: Option<Utf8PathBuf>,
    /// Output directory for the local review gallery built on drift.
    pub(crate) gallery: Option<Utf8PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capture: CaptureConfig::default(),
            comment: CommentConfig {
                title: "Visual changes".to_owned(),
                marker: "screencomp".to_owned(),
                show_unchanged: false,
                embed_limit: 10,
            },
            guard: GuardConfig::default(),
        }
    }
}

/// Errors from loading or validating configuration.
///
/// Re-exported at the crate root because it is reachable through
/// [`AppError`](crate::AppError).
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The requested config file does not exist.
    #[error("config file not found: {path}")]
    NotFound {
        /// Path that was requested.
        path: Utf8PathBuf,
    },

    /// The config file could not be read.
    #[error("failed to read config {path}")]
    Read {
        /// Path that could not be read.
        path: Utf8PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The config file was not valid TOML or had unexpected fields.
    #[error("failed to parse config {path}")]
    Parse {
        /// Path that failed to parse.
        path: Utf8PathBuf,
        /// Underlying TOML deserialization error.
        #[source]
        source: toml::de::Error,
    },

    /// The config parsed but failed semantic validation.
    #[error("invalid config: {reason}")]
    Invalid {
        /// Why the config was rejected.
        reason: String,
    },
}

/// Resolve and load configuration.
///
/// Precedence: `explicit` (`--config`) → `env` (`$SCREENCOMP_CONFIG`) →
/// `discovered` (an auto-found [`CONFIG_FILE`], already located by the caller) →
/// defaults. The first two are *explicit* and strict: a path that names a missing
/// file is a [`ConfigError::NotFound`]. `discovered` is the implicit fallback —
/// the caller passes `Some` only when the file exists, so absence is `None` and
/// yields defaults. `env` is read at the call boundary and passed in so this
/// function performs no ambient environment access.
pub(crate) fn load(
    explicit: Option<&Utf8Path>,
    env: Option<String>,
    discovered: Option<Utf8PathBuf>,
) -> Result<Config, ConfigError> {
    if let Some(path) = explicit {
        return load_file(path);
    }
    if let Some(path) = env.filter(|v| !v.is_empty()) {
        return load_file(Utf8Path::new(&path));
    }
    match discovered {
        Some(path) => load_file(&path),
        None => Ok(Config::default()),
    }
}

/// Read, parse, and validate a config file. A missing file is
/// [`ConfigError::NotFound`]; the caller decides whether that is reachable
/// (explicit sources) or pre-checked away (discovery).
fn load_file(path: &Utf8Path) -> Result<Config, ConfigError> {
    if !path.is_file() {
        return Err(ConfigError::NotFound {
            path: path.to_owned(),
        });
    }

    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_owned(),
        source,
    })?;

    let raw: RawConfig = toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_owned(),
        source,
    })?;

    raw.validate()
}

/// On-disk representation, mapped onto [`Config`] after validation.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    capture: RawCaptureConfig,
    #[serde(default)]
    comment: RawCommentConfig,
    #[serde(default)]
    guard: RawGuardConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCaptureConfig {
    arches: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommentConfig {
    title: Option<String>,
    marker: Option<String>,
    show_unchanged: Option<bool>,
    embed_limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGuardConfig {
    paths: Option<Vec<String>>,
    manifest: Option<Utf8PathBuf>,
    gallery: Option<Utf8PathBuf>,
}

impl RawConfig {
    fn validate(self) -> Result<Config, ConfigError> {
        let defaults = Config::default();
        let capture = self.capture.validate()?;
        let comment = self.comment;

        let title = comment.title.unwrap_or(defaults.comment.title);
        if title.trim().is_empty() {
            return Err(ConfigError::Invalid {
                reason: "comment.title must not be empty".to_owned(),
            });
        }

        let marker = comment.marker.unwrap_or(defaults.comment.marker);
        if marker.is_empty()
            || !marker
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ConfigError::Invalid {
                reason: "comment.marker must be non-empty and match [A-Za-z0-9_-]".to_owned(),
            });
        }

        let guard = self.guard.validate()?;

        Ok(Config {
            capture,
            comment: CommentConfig {
                title,
                marker,
                show_unchanged: comment
                    .show_unchanged
                    .unwrap_or(defaults.comment.show_unchanged),
                embed_limit: comment.embed_limit.unwrap_or(defaults.comment.embed_limit),
            },
            guard,
        })
    }
}

impl RawCaptureConfig {
    fn validate(self) -> Result<CaptureConfig, ConfigError> {
        let arches = self.arches.unwrap_or_default();
        if let Some(bad) = arches.iter().find(|a| a.trim().is_empty()) {
            return Err(ConfigError::Invalid {
                reason: format!("capture.arches contains an empty entry: {bad:?}"),
            });
        }
        Ok(CaptureConfig { arches })
    }
}

impl RawGuardConfig {
    fn validate(self) -> Result<GuardConfig, ConfigError> {
        let paths = self.paths.unwrap_or_default();
        if let Some(bad) = paths.iter().find(|p| p.trim().is_empty()) {
            return Err(ConfigError::Invalid {
                reason: format!("guard.paths contains an empty glob: {bad:?}"),
            });
        }

        Ok(GuardConfig {
            paths,
            manifest: self.manifest,
            gallery: self.gallery,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_path() {
        let cfg = load(None, None, None).expect("defaults load");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn empty_env_is_ignored() {
        let cfg = load(None, Some(String::new()), None).expect("empty env ignored");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn explicit_missing_is_error() {
        let err = load(Some(Utf8Path::new("/no/such/screencomp.toml")), None, None).unwrap_err();
        assert!(matches!(err, ConfigError::NotFound { .. }));
    }

    #[test]
    fn discovered_present_is_loaded_but_absent_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(tmp.path())
            .unwrap()
            .join("screencomp.toml");
        std::fs::write(&path, "[comment]\nmarker = \"discovered\"\n").unwrap();

        // A discovered file is parsed like any other.
        let cfg = load(None, None, Some(path)).expect("discovered loads");
        assert_eq!(cfg.comment.marker, "discovered");

        // An explicit source still wins over discovery.
        let tmp2 = tempfile::tempdir().unwrap();
        let explicit = Utf8Path::from_path(tmp2.path())
            .unwrap()
            .join("explicit.toml");
        std::fs::write(&explicit, "[comment]\nmarker = \"explicit\"\n").unwrap();
        let discovered = Utf8Path::from_path(tmp.path())
            .unwrap()
            .join("screencomp.toml");
        let cfg = load(Some(&explicit), None, Some(discovered)).expect("explicit wins");
        assert_eq!(cfg.comment.marker, "explicit");
    }

    #[test]
    fn rejects_blank_title() {
        let raw: RawConfig = toml::from_str("[comment]\ntitle = \"  \"\n").unwrap();
        assert!(matches!(raw.validate(), Err(ConfigError::Invalid { .. })));
    }

    #[test]
    fn rejects_bad_marker() {
        let raw: RawConfig = toml::from_str("[comment]\nmarker = \"has space\"\n").unwrap();
        assert!(matches!(raw.validate(), Err(ConfigError::Invalid { .. })));
    }

    #[test]
    fn rejects_unknown_field() {
        let err = toml::from_str::<RawConfig>("[comment]\nnope = true\n").unwrap_err();
        assert!(err.to_string().contains("nope") || err.to_string().contains("unknown"));
    }

    #[test]
    fn accepts_valid_overrides() {
        let raw: RawConfig = toml::from_str(
            "[comment]\ntitle = \"UI\"\nmarker = \"ui-shots\"\nshow_unchanged = true\nembed_limit = 3\n",
        )
        .unwrap();
        let cfg = raw.validate().expect("valid");
        assert_eq!(cfg.comment.title, "UI");
        assert_eq!(cfg.comment.marker, "ui-shots");
        assert!(cfg.comment.show_unchanged);
        assert_eq!(cfg.comment.embed_limit, 3);
    }

    #[test]
    fn embed_limit_defaults_to_ten() {
        let cfg = load(None, None, None).expect("defaults load");
        assert_eq!(cfg.comment.embed_limit, 10);
    }

    #[test]
    fn capture_arches_default_to_empty() {
        let cfg = load(None, None, None).expect("defaults load");
        assert!(cfg.capture.arches.is_empty());
    }

    #[test]
    fn accepts_capture_arches() {
        let raw: RawConfig =
            toml::from_str("[capture]\narches = [\"x86_64\", \"arm64\"]\n").unwrap();
        let cfg = raw.validate().expect("valid");
        assert_eq!(cfg.capture.arches, ["x86_64", "arm64"]);
    }

    #[test]
    fn rejects_empty_capture_arch() {
        let raw: RawConfig = toml::from_str("[capture]\narches = [\"arm64\", \"  \"]\n").unwrap();
        assert!(matches!(raw.validate(), Err(ConfigError::Invalid { .. })));
    }

    #[test]
    fn rejects_unknown_capture_field() {
        let err = toml::from_str::<RawConfig>("[capture]\nnope = true\n").unwrap_err();
        assert!(err.to_string().contains("nope") || err.to_string().contains("unknown"));
    }

    #[test]
    fn guard_defaults_to_empty() {
        let cfg = load(None, None, None).expect("defaults load");
        assert!(cfg.guard.paths.is_empty());
        assert_eq!(cfg.guard.manifest, None);
        assert_eq!(cfg.guard.gallery, None);
    }

    #[test]
    fn accepts_valid_guard() {
        let raw: RawConfig = toml::from_str(
            "[guard]\npaths = [\"src/**/*.rs\", \"playwright/**\"]\nmanifest = \"shots/baseline/x86_64.sha256\"\ngallery = \"shots/review\"\n",
        )
        .unwrap();
        let cfg = raw.validate().expect("valid");
        assert_eq!(cfg.guard.paths, ["src/**/*.rs", "playwright/**"]);
        assert_eq!(
            cfg.guard.manifest.as_deref().map(Utf8Path::as_str),
            Some("shots/baseline/x86_64.sha256")
        );
        assert_eq!(
            cfg.guard.gallery.as_deref().map(Utf8Path::as_str),
            Some("shots/review")
        );
    }

    #[test]
    fn rejects_empty_guard_glob() {
        let raw: RawConfig = toml::from_str("[guard]\npaths = [\"src/**\", \"  \"]\n").unwrap();
        assert!(matches!(raw.validate(), Err(ConfigError::Invalid { .. })));
    }

    #[test]
    fn rejects_unknown_guard_field() {
        let err = toml::from_str::<RawConfig>("[guard]\nnope = true\n").unwrap_err();
        assert!(err.to_string().contains("nope") || err.to_string().contains("unknown"));
    }
}
