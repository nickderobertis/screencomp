//! In-process integration tests.
//!
//! These drive the library entrypoint [`screencomp::run`] directly (parsing
//! `Cli` exactly as the binary does) and assert on the returned exit code,
//! captured stdout, on-disk effects, and typed error variants — without
//! spawning a subprocess. Full-process behavior is covered in `e2e.rs`.

use std::path::{Path, PathBuf};

use clap::Parser as _;
use screencomp::{AppError, Cli, run};
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn baseline() -> String {
    path_str(&fixtures().join("baseline"))
}

fn current() -> String {
    path_str(&fixtures().join("current"))
}

fn path_str(path: &Path) -> String {
    path.to_str().expect("fixture path is UTF-8").to_owned()
}

/// Parse `args` and run, capturing stdout. Panics if argument parsing fails so
/// tests of successful parsing stay terse.
fn invoke(args: &[&str]) -> (Result<i32, AppError>, String) {
    let cli = Cli::try_parse_from(args).expect("arguments parse");
    let mut out = Vec::new();
    let result = run(cli, &mut out);
    (result, String::from_utf8(out).expect("stdout is UTF-8"))
}

#[test]
fn classify_human_reports_changes_and_summary() {
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/about"), "{out}");
    assert!(out.contains("added desktop/pricing"), "{out}");
    assert!(
        out.contains("added 1 changed 1 removed 0 unchanged 2"),
        "{out}"
    );
}

#[test]
fn classify_json_is_single_line_contract() {
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--format",
        "json",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(out.contains(r#""changed":true"#), "{out}");
    assert!(
        out.contains(r#"{"project":"desktop","name":"pricing","status":"added"}"#),
        "{out}"
    );
    assert!(
        out.contains(r#""counts":{"added":1,"changed":1,"removed":0,"unchanged":2}"#),
        "{out}"
    );
}

#[test]
fn classify_exit_code_flag_signals_changes() {
    let (code, _) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
}

#[test]
fn quiet_suppresses_human_stdout() {
    let (code, out) = invoke(&[
        "screencomp",
        "-q",
        "classify",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.is_empty(), "quiet stdout should be empty: {out}");
}

#[test]
fn gallery_writes_index_html() {
    let out_dir = TempDir::new().unwrap();
    let out_str = path_str(out_dir.path());
    let (code, stdout) = invoke(&[
        "screencomp",
        "gallery",
        "--input",
        &current(),
        "--output",
        &out_str,
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(stdout.contains("wrote"));

    let html =
        std::fs::read_to_string(out_dir.path().join("index.html")).expect("index.html written");
    assert!(html.contains("<title>Screenshot gallery</title>"));
    assert!(html.contains("src=\"desktop/about.png\""));
}

#[test]
fn comment_writes_markdown_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("comment.md");
    let file_str = path_str(&file);
    let (code, _) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--output",
        &file_str,
    ]);
    assert_eq!(code.unwrap(), 0);

    let md = std::fs::read_to_string(&file).expect("comment file written");
    assert!(md.starts_with("<!-- screencomp -->"));
    assert!(md.contains("## Visual changes"));
    assert!(md.contains("### Changed\n- `desktop/about`"));
}

#[test]
fn missing_baseline_is_not_a_directory_error() {
    let (result, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        "/no/such/dir",
        "--current",
        &current(),
    ]);
    assert!(out.is_empty());
    assert!(matches!(result, Err(AppError::NotADirectory { .. })));
}

#[test]
fn explicit_missing_config_is_config_error() {
    let (result, _) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--config",
        "/no/such/screencomp.toml",
    ]);
    assert!(matches!(result, Err(AppError::Config(_))));
}

#[test]
fn valid_config_overrides_title_and_marker() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[comment]\ntitle = \"UI shots\"\nmarker = \"ui-shots\"\n",
    )
    .unwrap();
    let cfg_str = path_str(&cfg);

    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--config",
        &cfg_str,
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.starts_with("<!-- ui-shots -->"), "{out}");
    assert!(out.contains("## UI shots"), "{out}");
}
