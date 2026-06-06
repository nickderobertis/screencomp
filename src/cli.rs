//! Command-line surface.
//!
//! The entire CLI contract — subcommands, arguments, defaults, and environment
//! variable names — is declared here so it stays discoverable in one place.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};

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
pub struct ClassifyArgs {
    /// Baseline screenshot root (`<dir>/<project>/<name>.png`).
    #[arg(long, value_name = "DIR")]
    pub baseline: Utf8PathBuf,

    /// Current screenshot root to compare against the baseline.
    #[arg(long, value_name = "DIR")]
    pub current: Utf8PathBuf,

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

    /// Output directory; `index.html` is written inside it.
    #[arg(long, value_name = "DIR")]
    pub output: Utf8PathBuf,

    /// Page title for the generated gallery.
    #[arg(long, default_value = "Screenshot gallery")]
    pub title: String,
}

/// Arguments for [`Command::Comment`].
#[derive(Debug, clap::Args)]
pub struct CommentArgs {
    /// Baseline screenshot root.
    #[arg(long, value_name = "DIR")]
    pub baseline: Utf8PathBuf,

    /// Current screenshot root.
    #[arg(long, value_name = "DIR")]
    pub current: Utf8PathBuf,

    /// Optional `screencomp.toml`; falls back to `$SCREENCOMP_CONFIG`, then built-in defaults.
    #[arg(long, value_name = "FILE")]
    pub config: Option<Utf8PathBuf>,

    /// Optional gallery URL to link from the comment.
    #[arg(long, value_name = "URL")]
    pub gallery_url: Option<String>,

    /// Write the comment to this file instead of stdout.
    #[arg(long, value_name = "FILE")]
    pub output: Option<Utf8PathBuf>,
}
