//! Optional `screencomp.toml` configuration for the `comment` command.
//!
//! Every field has a default, so the tool runs without any config file. A file
//! is loaded only when requested explicitly (`--config`) or via the
//! [`CONFIG_ENV`] environment variable; in either case a missing or invalid file
//! is a hard error rather than a silent fallback.

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

/// Environment variable consulted for a config path when `--config` is absent.
pub(crate) const CONFIG_ENV: &str = "SCREENCOMP_CONFIG";

/// Validated configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    /// Settings for the rendered pull-request comment.
    pub(crate) comment: CommentConfig,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            comment: CommentConfig {
                title: "Visual changes".to_owned(),
                marker: "screencomp".to_owned(),
                show_unchanged: false,
            },
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
/// Precedence: `explicit` (`--config`) → `env` (`$SCREENCOMP_CONFIG`) → defaults.
/// `env` is read at the call boundary and passed in so this function performs no
/// ambient environment access.
pub(crate) fn load(
    explicit: Option<&Utf8Path>,
    env: Option<String>,
) -> Result<Config, ConfigError> {
    let path = match explicit {
        Some(p) => Some(p.to_owned()),
        None => env.filter(|v| !v.is_empty()).map(Utf8PathBuf::from),
    };

    let Some(path) = path else {
        return Ok(Config::default());
    };

    if !path.is_file() {
        return Err(ConfigError::NotFound { path });
    }

    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;

    let raw: RawConfig = toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })?;

    raw.validate()
}

/// On-disk representation, mapped onto [`Config`] after validation.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    comment: RawCommentConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommentConfig {
    title: Option<String>,
    marker: Option<String>,
    show_unchanged: Option<bool>,
}

impl RawConfig {
    fn validate(self) -> Result<Config, ConfigError> {
        let defaults = Config::default();
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

        Ok(Config {
            comment: CommentConfig {
                title,
                marker,
                show_unchanged: comment
                    .show_unchanged
                    .unwrap_or(defaults.comment.show_unchanged),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_path() {
        let cfg = load(None, None).expect("defaults load");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn empty_env_is_ignored() {
        let cfg = load(None, Some(String::new())).expect("empty env ignored");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn explicit_missing_is_error() {
        let err = load(Some(Utf8Path::new("/no/such/screencomp.toml")), None).unwrap_err();
        assert!(matches!(err, ConfigError::NotFound { .. }));
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
            "[comment]\ntitle = \"UI\"\nmarker = \"ui-shots\"\nshow_unchanged = true\n",
        )
        .unwrap();
        let cfg = raw.validate().expect("valid");
        assert_eq!(cfg.comment.title, "UI");
        assert_eq!(cfg.comment.marker, "ui-shots");
        assert!(cfg.comment.show_unchanged);
    }
}
