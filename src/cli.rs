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
