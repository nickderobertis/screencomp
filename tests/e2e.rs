//! End-to-end tests that execute the compiled `screencomp` binary.
//!
//! These cover critical user journeys from the user's perspective — exit codes,
//! stdout/stderr separation, and file effects — not just "the binary starts".

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("screencomp").expect("binary builds");
    // Keep tests hermetic regardless of the developer's environment.
    cmd.env_remove("SCREENCOMP_CONFIG");
    cmd
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn baseline() -> PathBuf {
    fixtures().join("baseline")
}

fn current() -> PathBuf {
    fixtures().join("current")
}

#[test]
fn help_lists_subcommands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: screencomp"))
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("gallery"))
        .stdout(predicate::str::contains("comment"));
}

#[test]
fn version_matches_crate() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(concat!(
            "screencomp ",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn classify_happy_path_separates_streams() {
    bin()
        .args(["classify", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("changed desktop/about"))
        .stdout(predicate::str::contains("added desktop/pricing"))
        .stdout(predicate::str::contains(
            "added 1 changed 1 removed 0 unchanged 2",
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn classify_json_contract() {
    bin()
        .args(["classify", "--format", "json", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""changed":true"#))
        .stdout(predicate::str::contains(r#""status":"added""#));
}

#[test]
fn classify_exit_code_flag_returns_three() {
    bin()
        .args(["classify", "--exit-code", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .code(3);
}

#[test]
fn quiet_writes_nothing_to_stdout() {
    bin()
        .args(["--quiet", "classify", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn comment_writes_file_and_reports_path() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("comment.md");

    bin()
        .args(["comment", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .arg("--output")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));

    let md = std::fs::read_to_string(&out).expect("comment file");
    assert!(md.starts_with("<!-- screencomp -->"));
    assert!(md.contains("## Visual changes"));
    assert!(md.contains("### Changed"));
}

#[test]
fn gallery_creates_index_html() {
    let dir = TempDir::new().unwrap();

    bin()
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--output")
        .arg(dir.path())
        .assert()
        .success();

    let html = std::fs::read_to_string(dir.path().join("index.html")).expect("index.html");
    assert!(html.contains("<title>Screenshot gallery</title>"));
    assert!(html.contains("src=\"desktop/about.png\""));

    // The gallery is self-contained: every referenced image is copied alongside
    // index.html with identical bytes, so the directory is deploy-ready.
    let copied = std::fs::read(dir.path().join("desktop/about.png")).expect("image copied");
    let source = std::fs::read(current().join("desktop/about.png")).expect("source image");
    assert_eq!(copied, source);
}

#[test]
fn gallery_diff_mode_renders_before_after() {
    let dir = TempDir::new().unwrap();

    bin()
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--baseline")
        .arg(baseline())
        .arg("--output")
        .arg(dir.path())
        .assert()
        .success();

    let html = std::fs::read_to_string(dir.path().join("index.html")).expect("index.html");
    assert!(html.contains("<h2>Changed</h2>"));
    assert!(html.contains("src=\"baseline/desktop/about.png\""));
    assert!(html.contains("src=\"current/desktop/about.png\""));
    // Both image trees are copied so before/after both render.
    assert!(dir.path().join("baseline/desktop/about.png").exists());
    assert!(dir.path().join("current/desktop/about.png").exists());
}

#[test]
fn missing_directory_fails_with_clean_stderr() {
    bin()
        .args(["classify", "--baseline", "/no/such/dir", "--current"])
        .arg(current())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not a directory"));
}

#[test]
fn missing_required_argument_is_usage_error() {
    bin()
        .arg("classify")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--baseline"));
}

#[test]
fn explicit_missing_config_fails() {
    bin()
        .args([
            "comment",
            "--config",
            "/no/such/screencomp.toml",
            "--baseline",
        ])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("config file not found"));
}

#[test]
fn config_from_flag_and_env_override_defaults() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[comment]\ntitle = \"UI shots\"\nmarker = \"ui-shots\"\n",
    )
    .unwrap();

    // Via --config flag.
    bin()
        .args(["comment", "--config"])
        .arg(&cfg)
        .arg("--baseline")
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("<!-- ui-shots -->"))
        .stdout(predicate::str::contains("## UI shots"));

    // Via SCREENCOMP_CONFIG environment variable.
    bin()
        .env("SCREENCOMP_CONFIG", &cfg)
        .args(["comment", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("<!-- ui-shots -->"));
}
