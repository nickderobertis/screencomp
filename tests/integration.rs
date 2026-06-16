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

/// Host CPU arch, mirroring `commands::arch::host_arch` (which is crate-private)
/// so the `--arch auto` journey can be exercised end to end.
fn host_arch() -> String {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
    .to_owned()
}

/// Write `bytes` to `<root>/<arch>/<project>/<name>.png`.
fn write_shot(root: &Path, arch: &str, project: &str, name: &str, bytes: &[u8]) {
    let dir = root.join(arch).join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.png")), bytes).unwrap();
}

/// Write a `screencomp.toml` with the given `[capture].arches`, returning its path.
fn write_arches_config(dir: &Path, arches: &[&str]) -> String {
    let list = arches
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let cfg = dir.join("screencomp.toml");
    std::fs::write(&cfg, format!("[capture]\narches = [{list}]\n")).unwrap();
    path_str(&cfg)
}

#[test]
fn arch_flag_scopes_comparison_to_one_subtree() {
    // Two arches coexist under each root. The `arm64` subtree is identical across
    // baseline and current; the `x86_64` subtree differs. Scoping to `arm64` must
    // report no changes even though `x86_64` would — proving cross-arch bytes
    // never leak into the comparison.
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");

    write_shot(&base, "arm64", "desktop", "home", b"same");
    write_shot(&cur, "arm64", "desktop", "home", b"same");
    write_shot(&base, "x86_64", "desktop", "home", b"old");
    write_shot(&cur, "x86_64", "desktop", "home", b"new");

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "arm64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("added 0 changed 0 removed 0 unchanged 1"),
        "{out}"
    );

    // The same roots scoped to the other arch do see the change.
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "x86_64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn arch_auto_resolves_to_the_host_subtree() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let key = host_arch();

    write_shot(&base, &key, "desktop", "home", b"old");
    write_shot(&cur, &key, "desktop", "home", b"new");

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "auto",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn arch_default_from_config_scopes_to_the_host_subtree() {
    // With `[capture].arches` listing the host arch and no `--arch`, commands
    // default to scoping by the host subtree.
    let dir = TempDir::new().unwrap();
    let key = host_arch();
    let cfg = write_arches_config(dir.path(), &[&key]);
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, &key, "desktop", "home", b"old");
    write_shot(&cur, &key, "desktop", "home", b"new");
    // A foreign subtree would differ but must be invisible to the scoped run.
    write_shot(&base, "other-arch", "desktop", "home", b"a");
    write_shot(&cur, "other-arch", "desktop", "home", b"b");

    let (code, out) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
    assert!(
        out.contains("added 0 changed 1 removed 0 unchanged 0"),
        "{out}"
    );
}

#[test]
fn arch_not_in_configured_arches_hard_errors() {
    // A config whose `[capture].arches` cannot contain the host arch, with no
    // explicit `--arch`, is a hard error pointing at the fix.
    let dir = TempDir::new().unwrap();
    let cfg = write_arches_config(dir.path(), &["sparc64"]);
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "sparc64", "desktop", "home", b"x");
    write_shot(&cur, "sparc64", "desktop", "home", b"x");

    let (result, _) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
    ]);
    let Err(err @ AppError::UnsupportedArch { .. }) = result else {
        panic!("expected UnsupportedArch, got {result:?}");
    };
    assert_eq!(err.exit_code(), 1);
    let msg = err.to_string();
    // The message must explain the problem, give the exact fix line (the existing
    // arch plus the host, ready to paste), and the CI-cost implication.
    assert!(msg.contains("is not in the configured arches"), "{msg}");
    assert!(
        msg.contains(&format!("arches = [\"sparc64\", \"{}\"]", host_arch())),
        "fix line should append the host arch: {msg}"
    );
    assert!(msg.contains("adds a CI job"), "{msg}");
}

#[test]
fn explicit_arch_overrides_configured_arches() {
    // An explicit `--arch` wins over `[capture].arches` even when the config
    // could not satisfy the host default on its own.
    let dir = TempDir::new().unwrap();
    let cfg = write_arches_config(dir.path(), &["sparc64"]);
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "x86_64", "desktop", "home", b"old");
    write_shot(&cur, "x86_64", "desktop", "home", b"new");

    let (code, out) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "x86_64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn arches_command_lists_configured_arches() {
    let dir = TempDir::new().unwrap();
    let cfg = write_arches_config(dir.path(), &["x86_64", "arm64"]);

    // Human: one arch per line.
    let (code, out) = invoke(&["screencomp", "--config", &cfg, "arches"]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(out, "x86_64\narm64\n", "{out}");

    // JSON: a single-line array.
    let (code, out) = invoke(&["screencomp", "--config", &cfg, "arches", "--format", "json"]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert_eq!(out.trim_end(), r#"["x86_64","arm64"]"#, "{out}");
}

#[test]
fn missing_arch_subtree_explains_the_layout() {
    // The baseline holds an `arm64` subtree but we ask for `x86_64`. The absent
    // subtree must produce a layout hint, not a bare "not a directory".
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "arm64", "desktop", "home", b"x");
    write_shot(&cur, "arm64", "desktop", "home", b"x");

    let (result, _) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "x86_64",
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected an InvalidLayout hint, got {result:?}");
    };
    assert!(reason.contains("x86_64"), "{reason}");
    // The hint points at the arch layer and what the root actually holds.
    assert!(reason.contains("--arch"), "{reason}");
    assert!(reason.contains("arm64"), "{reason}");
}

#[test]
fn arch_against_loose_pngs_hints_to_add_a_project_dir() {
    // Capture written flat (loose .png at the root) while --arch expects a
    // subtree: the hint must call out the loose files and the fix.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    std::fs::create_dir_all(&cur).unwrap();
    std::fs::write(cur.join("home.png"), b"x").unwrap();

    let (result, _) = invoke(&[
        "screencomp",
        "manifest",
        "--input",
        cur.to_str().unwrap(),
        "--arch",
        "x86_64",
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected an InvalidLayout hint, got {result:?}");
    };
    assert!(reason.contains("loose .png"), "{reason}");
    assert!(reason.contains("omit --arch"), "{reason}");
}

#[test]
fn comment_marker_and_title_flags_override_config() {
    // Distinct markers are how a multi-arch run keeps one sticky comment per arch
    // without a config file per arch.
    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline",
        &baseline(),
        "--current",
        &current(),
        "--marker",
        "screencomp-x86_64",
        "--title",
        "Visual changes (x86_64)",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.starts_with("<!-- screencomp-x86_64 -->"), "{out}");
    assert!(out.contains("## Visual changes (x86_64)"), "{out}");
}

#[test]
fn manifest_then_classify_against_it_matches_a_dir_baseline() {
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("baseline.sha256"));

    // Write a digest manifest of the baseline fixture.
    let (code, _) = invoke(&[
        "screencomp",
        "manifest",
        "--input",
        &baseline(),
        "--output",
        &manifest,
    ]);
    assert_eq!(code.unwrap(), 0);
    let body = std::fs::read_to_string(&manifest).unwrap();
    assert!(
        body.lines()
            .all(|l| l.contains("  ") && l.ends_with(".png")),
        "{body}"
    );

    // Classifying against the manifest yields the same result as the image dir.
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline-manifest",
        &manifest,
        "--current",
        &current(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("added 1 changed 1 removed 0 unchanged 2"),
        "{out}"
    );
    assert!(out.contains("changed desktop/about"), "{out}");
    assert!(out.contains("added desktop/pricing"), "{out}");
}

#[test]
fn comment_accepts_a_baseline_manifest() {
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.sha256"));
    invoke(&[
        "screencomp",
        "manifest",
        "--input",
        &baseline(),
        "--output",
        &manifest,
    ])
    .0
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline-manifest",
        &manifest,
        "--current",
        &current(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("### Changed\n- `desktop/about`"), "{out}");
}

#[test]
fn comment_manifest_mode_embeds_current_only_from_gallery_url() {
    // Manifest mode commits no baseline PNGs, so a `--gallery-url` (a plain
    // gallery of the current shots) must source "After" images from `<URL>/...`
    // and never emit a `baseline/` URL that would 404.
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.sha256"));
    invoke(&[
        "screencomp",
        "manifest",
        "--input",
        &baseline(),
        "--output",
        &manifest,
    ])
    .0
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline-manifest",
        &manifest,
        "--current",
        &current(),
        "--gallery-url",
        "https://example.test/site/",
    ]);
    assert_eq!(code.unwrap(), 0);
    // Plain layout, current shots only: no `baseline/` or `current/` segment.
    assert!(
        out.contains("src=\"https://example.test/site/desktop/about.png\""),
        "{out}"
    );
    assert!(!out.contains("/baseline/"), "{out}");
    assert!(!out.contains("/current/"), "{out}");
}

#[test]
fn comment_manifest_mode_sources_before_from_baseline_url() {
    // An explicit `--baseline-url` (a canonical/main gallery) restores a real
    // before/after diff in manifest mode: Before from it, After from --current-url.
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.sha256"));
    invoke(&[
        "screencomp",
        "manifest",
        "--input",
        &baseline(),
        "--output",
        &manifest,
    ])
    .0
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "comment",
        "--baseline-manifest",
        &manifest,
        "--current",
        &current(),
        "--baseline-url",
        "https://example.test/main",
        "--current-url",
        "https://example.test/pr/9",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("| Before | After |"), "{out}");
    assert!(
        out.contains("src=\"https://example.test/main/desktop/about.png\""),
        "{out}"
    );
    assert!(
        out.contains("src=\"https://example.test/pr/9/desktop/about.png\""),
        "{out}"
    );
}

#[test]
fn manifest_and_classify_are_arch_scoped() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "x86_64", "desktop", "home", b"v1");
    write_shot(&cur, "x86_64", "desktop", "home", b"v2");
    let manifest = path_str(&dir.path().join("x86_64.sha256"));

    invoke(&[
        "screencomp",
        "manifest",
        "--input",
        base.to_str().unwrap(),
        "--arch",
        "x86_64",
        "--output",
        &manifest,
    ])
    .0
    .unwrap();
    // The manifest drops the arch segment.
    assert_eq!(
        std::fs::read_to_string(&manifest)
            .unwrap()
            .lines()
            .next()
            .map(|l| l.ends_with("desktop/home.png")),
        Some(true)
    );

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline-manifest",
        &manifest,
        "--current",
        cur.to_str().unwrap(),
        "--arch",
        "x86_64",
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("changed desktop/home"), "{out}");
}

#[test]
fn malformed_manifest_is_invalid_layout_error() {
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("bad.sha256");
    std::fs::write(&manifest, "not-a-digest  desktop/home.png\n").unwrap();
    let (result, _) = invoke(&[
        "screencomp",
        "classify",
        "--baseline-manifest",
        manifest.to_str().unwrap(),
        "--current",
        &current(),
    ]);
    assert!(matches!(result, Err(AppError::InvalidLayout { .. })));
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

/// Write `bytes` to `<root>/<project>/<name>.png` (no platform layer).
fn write_flat(root: &Path, project: &str, name: &str, bytes: &[u8]) {
    let dir = root.join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.png")), bytes).unwrap();
}

#[test]
fn verify_identical_captures_pass() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_flat(&a, "desktop", "home", b"pixels");
    write_flat(&b, "desktop", "home", b"pixels");

    let (code, out) = invoke(&[
        "screencomp",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("reproducible: 1 shots byte-identical"),
        "{out}"
    );
}

#[test]
fn verify_divergent_captures_exit_three_with_kinds() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_flat(&a, "desktop", "home", b"v1"); // differs between runs
    write_flat(&b, "desktop", "home", b"v2");
    write_flat(&a, "desktop", "only_first", b"x"); // dropped in second run
    write_flat(&b, "desktop", "only_second", b"y"); // appeared in second run

    let (code, out) = invoke(&[
        "screencomp",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("differs desktop/home"), "{out}");
    assert!(out.contains("only-in-first desktop/only_first"), "{out}");
    assert!(out.contains("only-in-second desktop/only_second"), "{out}");
    assert!(
        out.contains("NOT reproducible: 1 differ, 1 only in first run, 1 only in second (of 3)"),
        "{out}"
    );
}

#[test]
fn verify_json_is_single_line_contract() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_flat(&a, "desktop", "home", b"v1");
    write_flat(&b, "desktop", "home", b"v2");

    let (code, out) = invoke(&[
        "screencomp",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(out.contains(r#""reproducible":false"#), "{out}");
    assert!(out.contains(r#""checked":1"#), "{out}");
    assert!(
        out.contains(r#"{"project":"desktop","name":"home","kind":"differs"}"#),
        "{out}"
    );
}

#[test]
fn verify_is_arch_scoped() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    let key = host_arch();
    write_shot(&a, &key, "desktop", "home", b"same");
    write_shot(&b, &key, "desktop", "home", b"same");
    // A foreign subtree diverges but must be invisible to the scoped run.
    write_shot(&a, "other-arch", "desktop", "home", b"p");
    write_shot(&b, "other-arch", "desktop", "home", b"q");

    let (code, out) = invoke(&[
        "screencomp",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
        "--arch",
        "auto",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("reproducible: 1 shots byte-identical"),
        "{out}"
    );
}

#[test]
fn doctor_reports_layout_and_passes_a_clean_tree() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    write_flat(&input, "desktop", "home", b"a");
    write_flat(&input, "desktop", "about", b"b");
    write_flat(&input, "mobile", "home", b"c");

    let (code, out) = invoke(&["screencomp", "doctor", "--input", input.to_str().unwrap()]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("arch: none (root is project-level)"), "{out}");
    assert!(out.contains("desktop (2 shots)"), "{out}");
    assert!(out.contains("mobile (1 shot)"), "{out}");
    assert!(out.contains("shots: 3"), "{out}");
    assert!(
        out.contains("ok: layout matches <project>/<name>.png"),
        "{out}"
    );
}

#[test]
fn doctor_resolves_auto_arch_key() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    let key = host_arch();
    write_shot(&input, &key, "desktop", "home", b"a");

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--arch",
        "auto",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(&format!("arch: {key} (auto)")), "{out}");
}

#[test]
fn doctor_arch_defaulted_from_config_shows_auto_suffix() {
    // No `--arch` but a configured host arch: doctor scopes to it and marks the
    // resolved key `(auto)` since it was host-detected, not named explicitly.
    let dir = TempDir::new().unwrap();
    let key = host_arch();
    let cfg = write_arches_config(dir.path(), &[&key]);
    let input = dir.path().join("current");
    write_shot(&input, &key, "desktop", "home", b"a");

    let (code, out) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "doctor",
        "--input",
        input.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(&format!("arch: {key} (auto)")), "{out}");
}

#[test]
fn doctor_explicit_arch_key_is_shown_without_auto_suffix() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    write_shot(&input, "x86_64", "desktop", "home", b"a");

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--arch",
        "x86_64",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("arch: x86_64"), "{out}");
    assert!(!out.contains("(auto)"), "{out}");
}

#[test]
fn doctor_json_contract_reports_problems() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    write_flat(&input, "desktop", "home", b"a");
    std::fs::write(input.join("stray.png"), b"oops").unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(out.contains(r#""ok":false"#), "{out}");
    assert!(out.contains(r#""total_shots":1"#), "{out}");
    assert!(out.contains(r#""loose_pngs":["stray.png"]"#), "{out}");
    assert!(out.contains(r#""arch":null"#), "{out}");
    assert!(out.contains(r#"{"name":"desktop","shots":1}"#), "{out}");
}

#[test]
fn verify_and_doctor_quiet_suppress_human_output() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_flat(&a, "desktop", "home", b"v1");
    write_flat(&b, "desktop", "home", b"v2");

    // Quiet still gates (exit 3) but writes nothing to stdout.
    let (code, out) = invoke(&[
        "screencomp",
        "-q",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.is_empty(), "quiet verify stdout should be empty: {out}");

    let (code, out) = invoke(&["screencomp", "-q", "doctor", "--input", a.to_str().unwrap()]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.is_empty(), "quiet doctor stdout should be empty: {out}");
}

#[test]
fn doctor_exit_code_flags_loose_pngs() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    std::fs::create_dir_all(&input).unwrap();
    // A capture stranded at the root instead of under a project directory.
    std::fs::write(input.join("home.png"), b"a").unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("warning:"), "{out}");
    assert!(out.contains("home.png"), "{out}");
    assert!(out.contains("problems found"), "{out}");
}

#[test]
fn doctor_missing_input_is_not_a_directory_error() {
    let (result, out) = invoke(&["screencomp", "doctor", "--input", "/no/such/dir"]);
    assert!(out.is_empty());
    assert!(matches!(result, Err(AppError::NotADirectory { .. })));
}

/// Write a `screencomp.toml` whose `[guard].paths` are `globs`, returning its path.
fn write_guard_config(dir: &Path, globs: &[&str]) -> String {
    let list = globs
        .iter()
        .map(|g| format!("{g:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let cfg = dir.join("screencomp.toml");
    std::fs::write(&cfg, format!("[guard]\npaths = [{list}]\n")).unwrap();
    path_str(&cfg)
}

/// Write `lines` (joined by newlines) to a candidate-paths file, returning its path.
fn write_changed(dir: &Path, lines: &[&str]) -> String {
    let file = dir.join("changed.txt");
    std::fs::write(&file, lines.join("\n")).unwrap();
    path_str(&file)
}

#[test]
fn scope_reports_relevant_match_and_gates_with_exit_code() {
    let dir = TempDir::new().unwrap();
    let cfg = write_guard_config(dir.path(), &["src/**/*.rs", "playwright/**"]);
    let changed = write_changed(
        dir.path(),
        &["README.md", "src/ui/button.rs", "playwright/home.spec.ts"],
    );

    let (code, out) = invoke(&[
        "screencomp",
        "scope",
        "--config",
        &cfg,
        "--changed-from",
        &changed,
        "--exit-code",
    ]);
    // A relevant path matched: exit 3, mirroring classify's "change → 3".
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("match src/ui/button.rs"), "{out}");
    assert!(out.contains("match playwright/home.spec.ts"), "{out}");
    assert!(!out.contains("README.md"), "{out}");
    assert!(
        out.contains("2 of 3 changed paths are screenshot-relevant"),
        "{out}"
    );
}

#[test]
fn scope_no_match_exits_zero() {
    let dir = TempDir::new().unwrap();
    let cfg = write_guard_config(dir.path(), &["src/**/*.rs"]);
    let changed = write_changed(dir.path(), &["README.md", "docs/guide.md"]);

    let (code, out) = invoke(&[
        "screencomp",
        "scope",
        "--config",
        &cfg,
        "--changed-from",
        &changed,
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("0 of 2 changed paths are screenshot-relevant"),
        "{out}"
    );
}

#[test]
fn scope_json_is_single_line_contract() {
    let dir = TempDir::new().unwrap();
    let cfg = write_guard_config(dir.path(), &["shots/**"]);
    let changed = write_changed(dir.path(), &["shots/baseline/x.sha256", "Cargo.toml"]);

    let (code, out) = invoke(&[
        "screencomp",
        "scope",
        "--config",
        &cfg,
        "--changed-from",
        &changed,
        "--format",
        "json",
    ]);
    // Without --exit-code the run always succeeds; the verdict is in the JSON.
    assert_eq!(code.unwrap(), 0);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(out.contains(r#""matched":true"#), "{out}");
    assert!(out.contains(r#""considered":2"#), "{out}");
    assert!(
        out.contains(r#""paths":["shots/baseline/x.sha256"]"#),
        "{out}"
    );
}

#[test]
fn scope_empty_input_never_matches() {
    let dir = TempDir::new().unwrap();
    let cfg = write_guard_config(dir.path(), &["src/**"]);
    // An empty file with only blank lines: no candidates, so no match.
    let changed = write_changed(dir.path(), &["", "  ", ""]);

    let (code, out) = invoke(&[
        "screencomp",
        "scope",
        "--config",
        &cfg,
        "--changed-from",
        &changed,
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("0 of 0 changed paths are screenshot-relevant"),
        "{out}"
    );
}

#[test]
fn scope_without_guard_paths_matches_nothing() {
    // Default config has no globs, so even a screenshot path is not relevant.
    let dir = TempDir::new().unwrap();
    let changed = write_changed(dir.path(), &["shots/current/desktop/home.png"]);

    let (code, out) = invoke(&[
        "screencomp",
        "scope",
        "--changed-from",
        &changed,
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("0 of 1 changed paths are screenshot-relevant"),
        "{out}"
    );
}

#[test]
fn scope_quiet_suppresses_human_output_but_still_gates() {
    let dir = TempDir::new().unwrap();
    let cfg = write_guard_config(dir.path(), &["src/**"]);
    let changed = write_changed(dir.path(), &["src/lib.rs"]);

    let (code, out) = invoke(&[
        "screencomp",
        "-q",
        "scope",
        "--config",
        &cfg,
        "--changed-from",
        &changed,
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.is_empty(), "quiet scope stdout should be empty: {out}");
}

#[test]
fn scope_missing_changed_file_is_io_error() {
    let (result, _) = invoke(&[
        "screencomp",
        "scope",
        "--changed-from",
        "/no/such/changed.txt",
    ]);
    assert!(matches!(result, Err(AppError::Io { .. })));
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

#[test]
fn init_scaffolds_config_workflow_and_gitignore() {
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());

    let (code, out) = invoke(&["screencomp", "init", "--dir", &root, "--arch", "arm64"]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("created") && out.contains("screencomp.toml"),
        "{out}"
    );

    // The config is valid and arch-substituted into [capture].arches.
    let toml = std::fs::read_to_string(dir.path().join("screencomp.toml")).unwrap();
    assert!(toml.contains("arches = [\"arm64\"]"), "{toml}");

    // The workflow calls the reusable workflow; the arch list lives in the config,
    // so the caller carries no `platform:`.
    let wf = std::fs::read_to_string(dir.path().join(".github/workflows/visual-docs.yml")).unwrap();
    assert!(
        wf.contains("nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v"),
        "{wf}"
    );
    assert!(!wf.contains("platform:"), "{wf}");

    // The .gitignore commits baselines but ignores generated images: no ignore
    // entry (a non-comment line) targets shots/baseline/.
    let ignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(ignore.contains("shots/current/"), "{ignore}");
    assert!(
        !ignore
            .lines()
            .any(|l| !l.starts_with('#') && l.contains("shots/baseline/")),
        "{ignore}"
    );
}

#[test]
fn init_is_idempotent_and_respects_force() {
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());
    let gitignore = dir.path().join(".gitignore");
    std::fs::write(&gitignore, "node_modules/\n").unwrap();

    invoke(&["screencomp", "init", "--dir", &root]).0.unwrap();
    // A second run leaves existing files untouched and does not duplicate the
    // .gitignore block.
    let (code, out) = invoke(&["screencomp", "init", "--dir", &root]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("skipped") && out.contains("screencomp.toml"),
        "{out}"
    );

    let ignore = std::fs::read_to_string(&gitignore).unwrap();
    assert!(ignore.starts_with("node_modules/"), "{ignore}");
    assert_eq!(
        ignore.matches("shots/current/").count(),
        1,
        "block must not duplicate: {ignore}"
    );

    // --force overwrites the config and workflow; the .gitignore block stays
    // skipped because its marker is already present (re-appending would dupe it).
    let (code, out) = invoke(&["screencomp", "init", "--dir", &root, "--force"]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.lines()
            .any(|l| l.starts_with("updated") && l.contains("screencomp.toml")),
        "{out}"
    );
}

#[test]
fn doctor_warns_on_a_cross_arch_baseline_manifest() {
    // A baseline manifest named for a different arch, against a capture where
    // every shot's bytes differ, is the "everything changed" trap doctor exists
    // to surface as an arch mismatch rather than a real diff.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_flat(&cur, "desktop", "home", b"new-bytes");

    // Manifest holds the same shot name but a different digest, named for an arch
    // that cannot be the host.
    let other = if host_arch() == "x86_64" {
        "arm64"
    } else {
        "x86_64"
    };
    let manifest = dir.path().join(format!("{other}.sha256"));
    std::fs::write(&manifest, format!("{}  desktop/home.png\n", "a".repeat(64))).unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        cur.to_str().unwrap(),
        "--baseline-manifest",
        manifest.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    // Both the filename heuristic and the all-differ heuristic fire.
    assert!(
        out.contains(&format!("baseline manifest '{other}'")),
        "{out}"
    );
    assert!(out.contains("every shared shot differs"), "{out}");

    // The JSON contract carries the same warnings.
    let (code, json) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        cur.to_str().unwrap(),
        "--baseline-manifest",
        manifest.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(json.lines().count(), 1, "JSON must be one line: {json}");
    assert!(json.contains(r#""warnings":["#), "{json}");
    assert!(json.contains("every shared shot differs"), "{json}");
}

#[test]
fn doctor_non_arch_manifest_name_skips_the_filename_warning() {
    // A manifest not named after an arch (baseline.sha256) must not trip the
    // filename heuristic, and a matching, identical shot leaves doctor clean.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_flat(&cur, "desktop", "home", b"pixels");

    // Generate a correct digest from the capture itself, named non-platform-like.
    let manifest = dir.path().join("baseline.sha256");
    invoke(&[
        "screencomp",
        "manifest",
        "--input",
        cur.to_str().unwrap(),
        "--output",
        manifest.to_str().unwrap(),
    ])
    .0
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        cur.to_str().unwrap(),
        "--baseline-manifest",
        manifest.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(!out.contains("baseline manifest"), "{out}");
    assert!(!out.contains("every shared shot differs"), "{out}");
    assert!(out.contains("ok: layout matches"), "{out}");
}

#[test]
fn init_defaults_to_the_host_arch() {
    // `init` with no `--arch` (and explicit `--arch auto`) scaffolds for the
    // host's CPU arch — the capture is a Linux container, so only the arch varies.
    // An ARM developer must get `arm64`, never the literal "auto".
    let key = host_arch();

    for arch in [None, Some("auto")] {
        let dir = TempDir::new().unwrap();
        let root = path_str(dir.path());
        let mut argv = vec!["screencomp", "init", "--dir", &root];
        if let Some(a) = arch {
            argv.extend(["--arch", a]);
        }
        let (code, _) = invoke(&argv);
        assert_eq!(code.unwrap(), 0);

        // The scaffold's [capture].arches is the resolved arch, not the literal "auto".
        let toml = std::fs::read_to_string(dir.path().join("screencomp.toml")).unwrap();
        assert!(toml.contains(&format!("arches = [\"{key}\"]")), "{toml}");
        assert!(!toml.contains("auto"), "{toml}");

        // The caller carries no per-arch input: the arch list lives in the config.
        let wf =
            std::fs::read_to_string(dir.path().join(".github/workflows/visual-docs.yml")).unwrap();
        assert!(!wf.contains("platform:"), "{wf}");
        assert!(!wf.contains("runs-on:"), "{wf}");
    }
}

#[test]
fn init_json_reports_each_file_action() {
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());
    let (code, out) = invoke(&["screencomp", "init", "--dir", &root, "--format", "json"]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(out.contains(r#""action":"created""#), "{out}");
    assert!(out.contains("screencomp.toml"), "{out}");
}

#[test]
fn init_caller_matches_the_reusable_workflow_interface() {
    // The scaffolded caller must stay consistent with the reusable workflow this
    // repo ships: a rename of an input/secret there (or moving the file) would
    // silently break every consumer's `init` output, and actionlint never lints
    // the runtime-generated caller, so guard the interface here.
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());
    invoke(&["screencomp", "init", "--dir", &root]).0.unwrap();
    let caller =
        std::fs::read_to_string(dir.path().join(".github/workflows/visual-docs.yml")).unwrap();

    let reusable_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/workflows/visual-docs-reusable.yml");
    let reusable = std::fs::read_to_string(&reusable_path)
        .expect("the reusable workflow the caller references must exist in this repo");

    // The caller's `uses:` points at that very file.
    assert!(
        caller.contains(".github/workflows/visual-docs-reusable.yml@"),
        "{caller}"
    );

    // Every `with:` input the caller passes is declared by the reusable workflow
    // (both indent inputs six spaces under their respective blocks). The strict
    // scaffold opts into `fail-on-drift` and `gh-pages-maintenance` explicitly, so
    // both must stay real inputs.
    for input in [
        "capture-command",
        "fail-on-drift",
        "gh-pages-maintenance",
        "gh-pages-history-versions",
    ] {
        let decl = format!("\n      {input}:");
        assert!(
            reusable.contains(&decl),
            "reusable workflow missing input {input}"
        );
        assert!(
            caller.contains(&decl),
            "caller stopped passing input {input}"
        );
    }
    // The strict scaffold does not auto-push the manifest, so it wires no
    // push-token; the secret stays declared for consumers who opt into
    // CI auto-accept (update-manifest: true).
    assert!(
        reusable.contains("\n      push-token:"),
        "reusable workflow missing secret push-token"
    );

    // gh-pages stays bounded only if the caller forwards the maintenance
    // triggers AND the reusable workflow has the jobs that act on them. Both
    // halves must move together or the bound silently breaks.
    assert!(
        caller.contains("closed]") && caller.contains("schedule:") && caller.contains("cron:"),
        "caller stopped forwarding the gh-pages cleanup/prune triggers:\n{caller}"
    );
    assert!(
        reusable.contains("cleanup-preview:") && reusable.contains("prune-history:"),
        "reusable workflow missing the gh-pages cleanup/prune jobs"
    );
}

#[test]
fn reusable_workflow_floats_its_own_action_pins() {
    // The reusable workflow references screencomp's own actions (install,
    // visual-docs, gh-pages-maintenance) by the floating major tag `@v0`, which
    // each release advances to itself (release.yml). `uses:` can't interpolate a
    // ref, so an exact `@vX.Y.Z` pin would silently go stale every release and a
    // brand-new action can't be referenced before it ships — `@v0` sidesteps both.
    // Guard against a regression back to exact pins.
    let reusable = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflows/visual-docs-reusable.yml"),
    )
    .unwrap();

    let mut refs = 0;
    for line in reusable.lines() {
        // Skip comments; match only `uses:` of screencomp's own actions.
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((_, after)) = line.split_once("uses: nickderobertis/screencomp") else {
            continue;
        };
        let Some((_, ref_part)) = after.split_once('@') else {
            continue;
        };
        let pin: String = ref_part
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        refs += 1;
        assert_eq!(
            pin, "v0",
            "internal action ref `@{pin}` should float on `@v0`, not an exact pin \
             (which goes stale every release): {line}"
        );
    }
    // install + visual-docs + cleanup + prune = 4 internal action references.
    assert!(
        refs >= 4,
        "expected 4 internal screencomp action refs; found {refs}"
    );
}
