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

    // The referenced image is copied next to index.html, byte-for-byte.
    let copied = std::fs::read(out_dir.path().join("desktop/about.png")).expect("image copied");
    let source = std::fs::read(std::path::Path::new(&current()).join("desktop/about.png"))
        .expect("source image");
    assert_eq!(copied, source);
}

#[test]
fn gallery_diff_mode_copies_both_trees() {
    let out_dir = TempDir::new().unwrap();
    let out_str = path_str(out_dir.path());
    let (code, stdout) = invoke(&[
        "screencomp",
        "gallery",
        "--input",
        &current(),
        "--baseline",
        &baseline(),
        "--output",
        &out_str,
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(stdout.contains("diff"));

    let html = std::fs::read_to_string(out_dir.path().join("index.html")).expect("index.html");
    assert!(html.contains("<h2>Changed</h2>"));
    assert!(out_dir.path().join("baseline/desktop/about.png").exists());
    assert!(out_dir.path().join("current/desktop/about.png").exists());
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
    // No base URL: a path listing, never inline images.
    assert!(!md.contains("<img"));
}

#[test]
fn comment_embeds_inline_previews_with_gallery_url() {
    // A small diff (1 changed + 1 added) plus a base URL embeds images inline.
    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--gallery-url",
        "https://example.test/pr/9/",
    ]);
    assert_eq!(code.unwrap(), 0);
    // Changed shot: before/after from both trees.
    assert!(out.contains("### Changed"), "{out}");
    assert!(
        out.contains("src=\"https://example.test/pr/9/baseline/desktop/about.png\""),
        "{out}"
    );
    assert!(
        out.contains("src=\"https://example.test/pr/9/current/desktop/about.png\""),
        "{out}"
    );
    // Added shot: single image from current.
    assert!(
        out.contains("src=\"https://example.test/pr/9/current/desktop/pricing.png\""),
        "{out}"
    );
    // Embed mode replaces the path listing but keeps the gallery link.
    assert!(!out.contains("- `desktop/about`"), "{out}");
    assert!(
        out.contains("[View full gallery](https://example.test/pr/9/)"),
        "{out}"
    );
}

#[test]
fn comment_embed_limit_zero_falls_back_to_listing() {
    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--gallery-url",
        "https://example.test/pr/9/",
        "--embed-limit",
        "0",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(!out.contains("<img"), "{out}");
    assert!(out.contains("### Changed\n- `desktop/about`"), "{out}");
}

/// Host platform key, mirroring `commands::platform::host_key` (which is
/// crate-private) so the `--platform auto` journey can be exercised end to end.
fn host_key() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// Write `bytes` to `<root>/<platform>/<project>/<name>.png`.
fn write_shot(root: &Path, platform: &str, project: &str, name: &str, bytes: &[u8]) {
    let dir = root.join(platform).join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.png")), bytes).unwrap();
}

#[test]
fn platform_flag_scopes_comparison_to_one_subtree() {
    // Two platforms coexist under each root. The `arm` subtree is identical
    // across baseline and current; the `x86` subtree differs. Scoping to `arm`
    // must report no changes even though `x86` would — proving cross-platform
    // bytes never leak into the comparison.
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");

    write_shot(&base, "linux-arm64", "desktop", "home", b"same");
    write_shot(&cur, "linux-arm64", "desktop", "home", b"same");
    write_shot(&base, "linux-x86_64", "desktop", "home", b"old");
    write_shot(&cur, "linux-x86_64", "desktop", "home", b"new");

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--platform",
        "linux-arm64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("added 0 changed 0 removed 0 unchanged 1"),
        "{out}"
    );

    // The same roots scoped to the other platform do see the change.
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--platform",
        "linux-x86_64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn platform_auto_resolves_to_the_host_subtree() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let key = host_key();

    write_shot(&base, &key, "desktop", "home", b"old");
    write_shot(&cur, &key, "desktop", "home", b"new");

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--platform",
        "auto",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn missing_platform_subtree_is_not_a_directory_error() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "linux-arm64", "desktop", "home", b"x");
    write_shot(&cur, "linux-arm64", "desktop", "home", b"x");

    let (result, _) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--platform",
        "windows-x86_64",
    ]);
    assert!(matches!(result, Err(AppError::NotADirectory { .. })));
}

#[test]
fn comment_marker_and_title_flags_override_config() {
    // Distinct markers are how a multi-platform run keeps one sticky comment per
    // platform without a config file per platform.
    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--marker",
        "screencomp-linux-x86_64",
        "--title",
        "Visual changes (linux-x86_64)",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.starts_with("<!-- screencomp-linux-x86_64 -->"), "{out}");
    assert!(out.contains("## Visual changes (linux-x86_64)"), "{out}");
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
