//! Command-line surface.
//!
//! The entire CLI contract — subcommands, arguments, defaults, and environment
//! variable names — is declared here so it stays discoverable in one place.

use camino::Utf8PathBuf;
use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

/// `screencomp` — byte-reproducible screenshot classification, gallery, and
/// pull-request comment rendering.
#[derive(Debug, Parser)]
#[command(name = "screencomp", version, about, long_about = None)]
pub struct Cli {
    /// Suppress non-essential human output; machine-readable output is unaffected.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Classify a current capture against a baseline (added/changed/removed/unchanged).
    Classify(ClassifyArgs),

    /// Render a static HTML gallery for a screenshot tree.
    Gallery(GalleryArgs),

    /// Render the sticky pull-request comment body for a classification.
    Comment(CommentArgs),

    /// Write a digest manifest for a screenshot tree, usable as a committed
    /// baseline that avoids storing the PNGs themselves.
    Manifest(ManifestArgs),

    /// Assert that two captures of the same build are byte-identical (the
    /// reproducibility gate); exit `3` if they diverge.
    Verify(VerifyArgs),

    /// Preflight a capture: print the resolved platform key and sanity-check the
    /// `<root>/<project>/<name>.png` layout before classifying.
    Doctor(DoctorArgs),

    /// Match a changed-path list against the `[guard].paths` globs to decide
    /// whether a screenshot-relevant file changed. Pure string matching: it
    /// reads no git, network, or working-tree state, so the local pre-push guard
    /// can use it to gate its (slow) re-capture.
    Scope(ScopeArgs),
}

/// Output encoding for commands that support both human and machine formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    /// Concise, human-readable lines.
    #[default]
    Human,
    /// Stable JSON document on a single line, suitable for automation.
    Json,
}

/// Arguments for [`Command::Classify`].
#[derive(Debug, clap::Args)]
#[command(group(ArgGroup::new("classify_baseline").required(true).args(["baseline", "baseline_manifest"])))]
pub struct ClassifyArgs {
    /// Baseline screenshot root (`<dir>/<project>/<name>.png`). Mutually
    /// exclusive with `--baseline-manifest`.
    #[arg(long, value_name = "DIR")]
    pub baseline: Option<Utf8PathBuf>,

    /// Baseline digest manifest (as written by `screencomp manifest`) to compare
    /// against instead of a `--baseline` image tree. Already platform-specific,
    /// so `--platform` then scopes only `--current`.
    #[arg(long, value_name = "FILE")]
    pub baseline_manifest: Option<Utf8PathBuf>,

    /// Current screenshot root to compare against the baseline.
    #[arg(long, value_name = "DIR")]
    pub current: Utf8PathBuf,

    /// Restrict the comparison to one platform subtree
    /// (`<root>/<platform>/<project>/<name>.png`), since identical UI rendered on
    /// a different OS or CPU architecture differs byte-for-byte. Use `auto` to
    /// detect the host `<os>-<arch>` (e.g. `linux-x86_64`, `macos-arm64`). Omit
    /// to treat the root as project-level, with no platform layer.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Exit with code 3 when any difference is detected (default: always 0 on success).
    #[arg(long)]
    pub exit_code: bool,
}

/// Arguments for [`Command::Gallery`].
#[derive(Debug, clap::Args)]
pub struct GalleryArgs {
    /// Screenshot root to index (`<dir>/<project>/<name>.png`). In diff mode this
    /// is the current capture.
    #[arg(long, value_name = "DIR")]
    pub input: Utf8PathBuf,

    /// Optional baseline root; when given, render a before/after diff gallery of
    /// `--input` against it instead of a plain index.
    #[arg(long, value_name = "DIR")]
    pub baseline: Option<Utf8PathBuf>,

    /// Restrict to one platform subtree (`<root>/<platform>/...`) of `--input`
    /// and, in diff mode, `--baseline`. Use `auto` to detect the host
    /// `<os>-<arch>`. Omit to treat the roots as project-level.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Output directory; `index.html` is written inside it.
    #[arg(long, value_name = "DIR")]
    pub output: Utf8PathBuf,

    /// Page title for the generated gallery.
    #[arg(long, default_value = "Screenshot gallery")]
    pub title: String,
}

/// Arguments for [`Command::Comment`].
#[derive(Debug, clap::Args)]
#[command(group(ArgGroup::new("comment_baseline").required(true).args(["baseline", "baseline_manifest"])))]
pub struct CommentArgs {
    /// Baseline screenshot root. Mutually exclusive with `--baseline-manifest`.
    #[arg(long, value_name = "DIR")]
    pub baseline: Option<Utf8PathBuf>,

    /// Baseline digest manifest (as written by `screencomp manifest`) to compare
    /// against instead of a `--baseline` image tree.
    #[arg(long, value_name = "FILE")]
    pub baseline_manifest: Option<Utf8PathBuf>,

    /// Current screenshot root.
    #[arg(long, value_name = "DIR")]
    pub current: Utf8PathBuf,

    /// Restrict the comparison to one platform subtree
    /// (`<root>/<platform>/<project>/<name>.png`). Use `auto` to detect the host
    /// `<os>-<arch>` (e.g. `linux-x86_64`, `macos-arm64`). Omit to treat the
    /// roots as project-level. Pair a per-platform `--marker` to keep one sticky
    /// comment each.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Optional `screencomp.toml`; falls back to `$SCREENCOMP_CONFIG`, then built-in defaults.
    #[arg(long, value_name = "FILE")]
    pub config: Option<Utf8PathBuf>,

    /// Heading shown at the top of the comment. Overrides `comment.title`.
    #[arg(long, value_name = "TEXT")]
    pub title: Option<String>,

    /// Stable HTML marker used to upsert the comment. Overrides `comment.marker`;
    /// give each platform a distinct value to keep one sticky comment per
    /// platform.
    #[arg(long, value_name = "ID")]
    pub marker: Option<String>,

    /// Optional gallery URL to link from the comment. When set, it is also the
    /// base URL for inline image previews.
    #[arg(long, value_name = "URL")]
    pub gallery_url: Option<String>,

    /// Embed inline image previews when at most this many screenshots differ
    /// (requires `--gallery-url`). Overrides `comment.embed_limit` (default 10);
    /// `0` disables embedding.
    #[arg(long, value_name = "N")]
    pub embed_limit: Option<usize>,

    /// Write the comment to this file instead of stdout.
    #[arg(long, value_name = "FILE")]
    pub output: Option<Utf8PathBuf>,
}

/// Arguments for [`Command::Manifest`].
#[derive(Debug, clap::Args)]
pub struct ManifestArgs {
    /// Screenshot root to digest (`<dir>/<project>/<name>.png`).
    #[arg(long, value_name = "DIR")]
    pub input: Utf8PathBuf,

    /// Restrict to one platform subtree (`<root>/<platform>/...`) of `--input`.
    /// Use `auto` to detect the host `<os>-<arch>`. Omit to treat the root as
    /// project-level. The written manifest never includes the platform segment.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Write the manifest to this file instead of stdout.
    #[arg(long, value_name = "FILE")]
    pub output: Option<Utf8PathBuf>,
}

/// Arguments for [`Command::Verify`].
#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// First capture of the build (`<dir>/<project>/<name>.png`).
    #[arg(long, value_name = "DIR")]
    pub first: Utf8PathBuf,

    /// Second capture of the *same* build, expected byte-identical to `--first`.
    #[arg(long, value_name = "DIR")]
    pub second: Utf8PathBuf,

    /// Restrict the comparison to one platform subtree
    /// (`<root>/<platform>/<project>/<name>.png`) of both captures. Use `auto`
    /// to detect the host `<os>-<arch>`. Omit to treat the roots as
    /// project-level.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,
}

/// Arguments for [`Command::Doctor`].
#[derive(Debug, clap::Args)]
pub struct DoctorArgs {
    /// Capture root to inspect (`<dir>/<project>/<name>.png`).
    #[arg(long, value_name = "DIR")]
    pub input: Utf8PathBuf,

    /// Resolve and inspect a single platform subtree
    /// (`<root>/<platform>/<project>/<name>.png`). Use `auto` to detect the host
    /// `<os>-<arch>` (the resolved key is printed). Omit to treat the root as
    /// project-level.
    #[arg(long, value_name = "KEY")]
    pub platform: Option<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Exit with code 3 when the layout has problems (empty capture or `.png`
    /// files stranded at the root), for use as a CI preflight gate.
    #[arg(long)]
    pub exit_code: bool,
}

/// Arguments for [`Command::Scope`].
#[derive(Debug, clap::Args)]
pub struct ScopeArgs {
    /// Read the newline-delimited candidate paths from this file, or `-` for
    /// standard input (the default). The pre-push hook pipes `git diff
    /// --name-only` in on stdin; blank lines are ignored.
    #[arg(long, value_name = "FILE", default_value = "-")]
    pub changed_from: Utf8PathBuf,

    /// Optional `screencomp.toml` providing `[guard].paths`; falls back to
    /// `$SCREENCOMP_CONFIG`, then built-in defaults (no globs, so nothing
    /// matches).
    #[arg(long, value_name = "FILE")]
    pub config: Option<Utf8PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Exit with code 3 when at least one candidate path matches `[guard].paths`
    /// (mirroring `classify --exit-code`); exit 0 when none match. Lets the hook
    /// branch on the exit status without parsing output.
    #[arg(long)]
    pub exit_code: bool,
}
