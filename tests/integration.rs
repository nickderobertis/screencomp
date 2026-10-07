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

mod common;
use common::command;

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

/// A 64-hex digest from a single repeated byte, e.g. `digest("aa")`.
fn digest(seed: &str) -> String {
    seed.repeat(64 / seed.len())
}

/// One shot to write into a capture index: `(name, toggles, hash,
/// image_filename, image_bytes)`.
type ShotSpec<'a> = (
    &'a str,
    &'a [(&'a str, &'a str)],
    &'a str,
    &'a str,
    &'a [u8],
);

/// Write a `captures.json` index (and its referenced images) into `dir`.
///
/// The directory is created as needed; this is the new capture shape that
/// replaced the old `<project>/<name>.png` tree.
fn write_capture(dir: &Path, shots: &[ShotSpec<'_>]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut entries = Vec::new();
    for (name, toggles, hash, image, bytes) in shots {
        std::fs::write(dir.join(image), bytes).unwrap();
        let tg: Vec<String> = toggles
            .iter()
            .map(|(k, v)| format!("\"{k}\":\"{v}\""))
            .collect();
        entries.push(format!(
            "{{\"name\":\"{name}\",\"toggles\":{{{}}},\"hash\":\"{hash}\",\"image\":\"{image}\"}}",
            tg.join(",")
        ));
    }
    std::fs::write(
        dir.join("captures.json"),
        format!("{{\"schema\":1,\"shots\":[{}]}}", entries.join(",")),
    )
    .unwrap();
}

/// Write a single-shot capture under `dir`, the common helper for the many
/// scoping/error tests that only need one shot.
fn write_one(dir: &Path, name: &str, hash: &str, bytes: &[u8]) {
    write_capture(
        dir,
        &[(name, &[("viewport", "desktop")], hash, "home.png", bytes)],
    );
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
    assert!(out.contains("changed about [viewport=desktop]"), "{out}");
    assert!(out.contains("added pricing [viewport=desktop]"), "{out}");
    assert!(
        out.contains("added 1 changed 1 removed 0 unchanged 2"),
        "{out}"
    );
    // A `changed` shot earns the cross-CPU-drift hint so a flaky gate points the
    // reader at the diagnosis instead of a multi-hour spelunk.
    assert!(
        out.contains("cross-CPU anti-aliasing drift") && out.contains("deviceScaleFactor"),
        "{out}"
    );
}

#[test]
fn classify_hint_only_fires_on_changed_shots() {
    // Added/removed (but nothing `changed`) is not how cross-CPU AA drift shows
    // up, so the hint must stay quiet — it only fires on a byte-different shot.
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let vp: &[(&str, &str)] = &[("viewport", "desktop")];
    write_capture(&base, &[("home", vp, &digest("aa"), "home.png", b"home")]);
    write_capture(
        &cur,
        &[
            ("home", vp, &digest("aa"), "home.png", b"home"), // unchanged
            ("pricing", vp, &digest("dd"), "pricing.png", b"new"), // added only
        ],
    );

    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline",
        base.to_str().unwrap(),
        "--current",
        cur.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("added 1 changed 0 removed 0 unchanged 1"),
        "{out}"
    );
    assert!(!out.contains("cross-CPU"), "hint must not fire: {out}");
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
    // The JSON carries the name and the toggle map, no more `project` field.
    assert!(
        out.contains(r#"{"name":"pricing","toggles":{"viewport":"desktop"},"status":"added"}"#),
        "{out}"
    );
    assert!(
        out.contains(r#"{"name":"about","toggles":{"viewport":"desktop"},"status":"changed"}"#),
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
    // One card per name, image referenced by its relative path.
    assert!(html.contains("<h2>about</h2>"), "{html}");
    assert!(html.contains("src=\"about-desktop.png\""), "{html}");

    // The referenced image is copied next to index.html, byte-for-byte.
    let copied = std::fs::read(out_dir.path().join("about-desktop.png")).expect("image copied");
    let source = std::fs::read(std::path::Path::new(&current()).join("about-desktop.png"))
        .expect("source image");
    assert_eq!(copied, source);
    assert_eq!(
        std::fs::read(out_dir.path().join("captures.json")).unwrap(),
        std::fs::read(std::path::Path::new(&current()).join("captures.json")).unwrap()
    );
}

#[test]
fn gallery_renders_a_toggle_control_per_declared_dimension() {
    // With a `viewport` dimension declared, the `home` name (which has both
    // desktop and mobile present) gets a control; `about`/`pricing` (one value)
    // do not. The control buttons carry `data-dim`/`data-val`, and the variant
    // images carry `data-variant` keyed by the toggle.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nlabel = \"Viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();
    let out_dir = TempDir::new().unwrap();
    let out_str = path_str(out_dir.path());

    let (code, _) = invoke(&[
        "screencomp",
        "--config",
        cfg.to_str().unwrap(),
        "gallery",
        "--input",
        &current(),
        "--output",
        &out_str,
    ]);
    assert_eq!(code.unwrap(), 0);

    let html = std::fs::read_to_string(out_dir.path().join("index.html")).expect("index.html");
    assert!(html.contains("data-dim=\"viewport\""), "{html}");
    assert!(html.contains("data-val=\"desktop\""), "{html}");
    assert!(html.contains("data-val=\"mobile\""), "{html}");
    assert!(html.contains("data-variant=\"viewport=desktop\""), "{html}");
    assert!(html.contains("data-variant=\"viewport=mobile\""), "{html}");
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
    assert!(out_dir.path().join("baseline/about-desktop.png").exists());
    assert!(out_dir.path().join("current/about-desktop.png").exists());
    assert!(out_dir.path().join("baseline/captures.json").exists());
    assert!(out_dir.path().join("current/captures.json").exists());
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
    assert!(md.contains("### Changed\n- `about [viewport=desktop]`"));
    // No base URL: a label listing, never inline images.
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
    // Changed shot: before/after from both trees (URLs are `<base>/<image>`).
    assert!(out.contains("### Changed"), "{out}");
    assert!(
        out.contains("src=\"https://example.test/pr/9/baseline/about-desktop.png\""),
        "{out}"
    );
    assert!(
        out.contains("src=\"https://example.test/pr/9/current/about-desktop.png\""),
        "{out}"
    );
    // Added shot: single image from current.
    assert!(
        out.contains("src=\"https://example.test/pr/9/current/pricing-desktop.png\""),
        "{out}"
    );
    // Embed mode replaces the label listing but keeps the gallery link.
    assert!(!out.contains("- `about [viewport=desktop]`"), "{out}");
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
    assert!(
        out.contains("### Changed\n- `about [viewport=desktop]`"),
        "{out}"
    );
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

    write_one(&base.join("arm64"), "home", &digest("aa"), b"same");
    write_one(&cur.join("arm64"), "home", &digest("aa"), b"same");
    write_one(&base.join("x86_64"), "home", &digest("bb"), b"old");
    write_one(&cur.join("x86_64"), "home", &digest("cc"), b"new");

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
    assert!(out.contains("changed home [viewport=desktop]"), "{out}");
}

#[test]
fn arch_auto_resolves_to_the_host_subtree() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let key = host_arch();

    write_one(&base.join(&key), "home", &digest("bb"), b"old");
    write_one(&cur.join(&key), "home", &digest("cc"), b"new");

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
    assert!(out.contains("changed home [viewport=desktop]"), "{out}");
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
    write_one(&base.join(&key), "home", &digest("bb"), b"old");
    write_one(&cur.join(&key), "home", &digest("cc"), b"new");
    // A foreign subtree would differ but must be invisible to the scoped run.
    write_one(&base.join("other-arch"), "home", &digest("11"), b"a");
    write_one(&cur.join("other-arch"), "home", &digest("22"), b"b");

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
    assert!(out.contains("changed home [viewport=desktop]"), "{out}");
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
    write_one(&base.join("sparc64"), "home", &digest("aa"), b"x");
    write_one(&cur.join("sparc64"), "home", &digest("aa"), b"x");

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
    write_one(&base.join("x86_64"), "home", &digest("bb"), b"old");
    write_one(&cur.join("x86_64"), "home", &digest("cc"), b"new");

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
    assert!(out.contains("changed home [viewport=desktop]"), "{out}");
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
    write_one(&base.join("arm64"), "home", &digest("aa"), b"x");
    write_one(&cur.join("arm64"), "home", &digest("aa"), b"x");

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
    // The hint points at the arch layer and the expected captures.json path.
    assert!(reason.contains("--arch"), "{reason}");
    assert!(reason.contains("captures.json"), "{reason}");
}

#[test]
fn capture_dir_without_index_is_invalid_layout() {
    // A capture directory written without its `captures.json` (the wrong-path
    // mistake a capture step makes) is an InvalidLayout naming the missing file.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    std::fs::create_dir_all(&cur).unwrap();
    std::fs::write(cur.join("home.png"), b"x").unwrap();

    let (result, _) = invoke(&["screencomp", "manifest", "--input", cur.to_str().unwrap()]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected an InvalidLayout hint, got {result:?}");
    };
    assert!(reason.contains("captures.json"), "{reason}");
}

/// Write `bytes` to `<dir>/<relative>`, creating parent directories: a captured
/// PNG as a capture step leaves it, with no index beside it yet.
fn write_png(dir: &Path, relative: &str, bytes: &[u8]) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Read the index `index` wrote under `dir`.
fn read_index(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("captures.json")).expect("index was written")
}

#[test]
fn index_names_flat_shots_after_their_relative_path() {
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "home.png", b"home-bytes");
    write_png(&cur, "checkout/step-2.png", b"step-bytes");
    // Not a screenshot: ignored rather than indexed as a shot.
    write_png(&cur, "notes.txt", b"ignore me");

    let (code, out) = invoke(&["screencomp", "index", "--input", cur.to_str().unwrap()]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("captures.json") && out.contains("2 shots"),
        "{out}"
    );

    let index = read_index(&cur);
    assert!(index.contains("\"name\": \"home\""), "{index}");
    assert!(index.contains("\"name\": \"checkout/step-2\""), "{index}");
    assert!(
        index.contains("\"image\": \"checkout/step-2.png\""),
        "{index}"
    );
    assert!(!index.contains("notes"), "{index}");
    // Each digest is the plain hex SHA-256 of the file's bytes, so a capture step
    // that keeps hashing its own screenshots (`sha256sum`, `createHash('sha256')`)
    // stays interchangeable with this command.
    assert!(
        index.contains(
            "\"hash\": \"1891a401bb3964f6ec7f7f05cd69cc073e8ce89b456185b04941331d60b2c77b\""
        ),
        "sha256 of home-bytes: {index}"
    );
    assert!(
        index.contains(
            "\"hash\": \"5aafe9fee23bd36796f85d19817cd1ae3284d5ded8e38dd16d67bb6aa937a530\""
        ),
        "sha256 of step-bytes: {index}"
    );
}

#[test]
fn index_reads_toggles_from_path_segments() {
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "theme=dark/home.png", b"dark-home");
    write_png(&cur, "home/viewport=mobile.png", b"mobile-home");

    let (code, _) = invoke(&[
        "screencomp",
        "index",
        "--input",
        cur.to_str().unwrap(),
        "--toggles-from-path",
        "--toggle",
        "project=shop",
    ]);
    assert_eq!(code.unwrap(), 0);

    // Both shots collapse onto the name `home`, each carrying its path toggle plus
    // the fixed one every shot gets.
    let index = read_index(&cur);
    assert_eq!(index.matches("\"name\": \"home\"").count(), 2, "{index}");
    assert!(index.contains("\"theme\": \"dark\""), "{index}");
    assert!(index.contains("\"viewport\": \"mobile\""), "{index}");
    assert_eq!(index.matches("\"project\": \"shop\"").count(), 2, "{index}");
}

#[test]
fn index_rejects_two_paths_that_name_one_shot() {
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "theme=dark/home.png", b"one");
    write_png(&cur, "home/theme=dark.png", b"two");

    let (result, _) = invoke(&[
        "screencomp",
        "index",
        "--input",
        cur.to_str().unwrap(),
        "--toggles-from-path",
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected InvalidLayout, got {result:?}");
    };
    assert!(reason.contains("home [theme=dark]"), "{reason}");
}

#[test]
fn index_rejects_a_path_toggle_that_contradicts_a_fixed_one() {
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "theme=light/home.png", b"one");

    let (result, _) = invoke(&[
        "screencomp",
        "index",
        "--input",
        cur.to_str().unwrap(),
        "--toggles-from-path",
        "--toggle",
        "theme=dark",
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected InvalidLayout, got {result:?}");
    };
    assert!(reason.contains("set twice"), "{reason}");
}

#[test]
fn index_rejects_one_toggle_key_given_two_values() {
    // Last-one-wins would index a pass the caller never meant to describe.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "home.png", b"one");

    let (result, _) = invoke(&[
        "screencomp",
        "index",
        "--input",
        cur.to_str().unwrap(),
        "--toggle",
        "theme=dark",
        "--toggle",
        "theme=light",
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected InvalidLayout, got {result:?}");
    };
    assert!(reason.contains("twice"), "{reason}");

    // Repeating the same assignment is harmless.
    let (code, _) = invoke(&[
        "screencomp",
        "index",
        "--input",
        cur.to_str().unwrap(),
        "--toggle",
        "theme=dark",
        "--toggle",
        "theme=dark",
    ]);
    assert_eq!(code.unwrap(), 0);
}

#[test]
fn index_of_a_tree_without_screenshots_is_invalid_layout() {
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    std::fs::create_dir_all(&cur).unwrap();

    let (result, _) = invoke(&["screencomp", "index", "--input", cur.to_str().unwrap()]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected InvalidLayout, got {result:?}");
    };
    assert!(reason.contains("no .png files"), "{reason}");
}

#[test]
fn index_of_a_missing_root_is_not_a_directory_error() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope");

    let (result, _) = invoke(&["screencomp", "index", "--input", missing.to_str().unwrap()]);
    assert!(
        matches!(result, Err(AppError::NotADirectory { .. })),
        "{result:?}"
    );
}

#[test]
fn index_defaults_to_the_host_arch_subtree_from_config() {
    // With `[capture].arches` configured, `index` resolves the arch exactly like
    // every other command: it writes `<root>/<arch>/captures.json`.
    let dir = TempDir::new().unwrap();
    let cfg = write_arches_config(dir.path(), &[&host_arch()]);
    let cur = dir.path().join("current");
    write_png(&cur.join(host_arch()), "home.png", b"host-bytes");

    let (code, out) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "index",
        "--input",
        cur.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(&host_arch()), "{out}");
    assert!(cur.join(host_arch()).join("captures.json").is_file());
    assert!(!cur.join("captures.json").exists(), "root stays index-free");
}

#[test]
fn index_missing_arch_subtree_explains_the_layout() {
    // The capture wrote its PNGs flat at the root while the config expects an
    // arch subtree — the mistake the arch layer invites.
    let dir = TempDir::new().unwrap();
    let cfg = write_arches_config(dir.path(), &[&host_arch()]);
    let cur = dir.path().join("current");
    write_png(&cur, "home.png", b"flat");

    let (result, _) = invoke(&[
        "screencomp",
        "--config",
        &cfg,
        "index",
        "--input",
        cur.to_str().unwrap(),
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected an InvalidLayout hint, got {result:?}");
    };
    assert!(reason.contains(&host_arch()), "{reason}");
    assert!(reason.contains("--arch"), "{reason}");
}

#[test]
fn index_is_byte_stable_across_runs() {
    // Re-indexing an unchanged capture rewrites an identical file, so an index
    // committed alongside a capture never churns.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_png(&cur, "b.png", b"bee");
    write_png(&cur, "a.png", b"ay");

    invoke(&["screencomp", "index", "--input", cur.to_str().unwrap()])
        .0
        .unwrap();
    let first = read_index(&cur);
    invoke(&["screencomp", "index", "--input", cur.to_str().unwrap()])
        .0
        .unwrap();
    assert_eq!(first, read_index(&cur));
    // Shots are emitted in name order regardless of directory-read order.
    assert!(
        first.find("\"a\"").unwrap() < first.find("\"b\"").unwrap(),
        "{first}"
    );
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
    let manifest = path_str(&dir.path().join("baseline.json"));

    // Write a JSON digest baseline of the baseline fixture.
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
    // Pretty-printed schema-1 index, digests present, images stripped.
    assert!(body.contains("\"schema\": 1"), "{body}");
    assert!(body.contains("\"hash\""), "{body}");
    assert!(!body.contains("\"image\""), "baseline drops images: {body}");
    assert!(body.ends_with('\n'), "{body}");

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
    assert!(out.contains("changed about [viewport=desktop]"), "{out}");
    assert!(out.contains("added pricing [viewport=desktop]"), "{out}");
}

#[test]
fn manifest_baseline_round_trips_as_a_parseable_index() {
    // The written baseline is itself a captures.json-shaped index: re-reading it
    // as a `--baseline-manifest` reproduces the same digests, so a committed
    // baseline is a self-describing artifact (no separate parser).
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("baseline.json"));
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

    // Comparing the baseline against itself via the manifest sees no changes.
    let (code, out) = invoke(&[
        "screencomp",
        "classify",
        "--baseline-manifest",
        &manifest,
        "--current",
        &baseline(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("added 0 changed 0 removed 0 unchanged 3"),
        "{out}"
    );
}

#[test]
fn comment_accepts_a_baseline_manifest() {
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.json"));
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
    assert!(
        out.contains("### Changed\n- `about [viewport=desktop]`"),
        "{out}"
    );
}

#[test]
fn comment_manifest_mode_embeds_current_only_from_gallery_url() {
    // Manifest mode commits no baseline PNGs, so a `--gallery-url` (a plain
    // gallery of the current shots) must source "After" images from `<URL>/...`
    // and never emit a `baseline/` URL that would 404.
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.json"));
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
        out.contains("src=\"https://example.test/site/about-desktop.png\""),
        "{out}"
    );
    assert!(!out.contains("/baseline/"), "{out}");
    assert!(!out.contains("/current/"), "{out}");
}

#[test]
fn comment_manifest_mode_sources_before_from_a_separate_baseline_url() {
    // A baseline manifest commits no image paths (stripped on write), but pointing
    // `--baseline-url` at a separate canonical gallery that hosts the same shot at
    // the same relative path restores a real before/after diff: "Before" from that
    // base, "After" from --current-url.
    let dir = TempDir::new().unwrap();
    let manifest = path_str(&dir.path().join("b.json"));
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
        out.contains("src=\"https://example.test/main/about-desktop.png\""),
        "{out}"
    );
    assert!(
        out.contains("src=\"https://example.test/pr/9/about-desktop.png\""),
        "{out}"
    );
}

/// Build a two-project affected set under `root` and return the path to a
/// `--projects` spec that aggregates them. `app-web` uses a digest-manifest
/// baseline (home changed, pricing added); `app-admin` uses an image-tree baseline
/// (about removed, home unchanged) and overrides its display label.
fn write_aggregate_spec(root: &Path) -> String {
    let vp: &[(&str, &str)] = &[("viewport", "desktop")];

    // app-web: manifest baseline vs a current with a changed + an added shot.
    write_capture(
        &root.join("app-web/current"),
        &[
            ("home", vp, &digest("11"), "home.png", b"new"),
            ("pricing", vp, &digest("22"), "pricing.png", b"add"),
        ],
    );
    let web_manifest = root.join("app-web/baseline.json");
    std::fs::write(
        &web_manifest,
        format!(
            r#"{{"schema":1,"shots":[{{"name":"home","toggles":{{"viewport":"desktop"}},"hash":"{}"}}]}}"#,
            digest("33")
        ),
    )
    .unwrap();

    // app-admin: image-tree baseline (home + about) vs a current missing `about`.
    write_capture(
        &root.join("app-admin/baseline"),
        &[
            ("home", vp, &digest("aa"), "home.png", b"h"),
            ("about", vp, &digest("bb"), "about.png", b"a"),
        ],
    );
    write_capture(
        &root.join("app-admin/current"),
        &[("home", vp, &digest("aa"), "home.png", b"h")],
    );

    let spec = root.join("projects.json");
    std::fs::write(
        &spec,
        format!(
            r#"{{
              "schema": 2,
              "projects": [
                {{
                  "id": "app-web",
                  "current": {web_current:?},
                  "baseline_manifest": {web_manifest:?},
                  "gallery_url": "https://example.test/pr-1/app-web",
                  "baseline_url": "https://example.test/pr-1/app-web/baseline",
                  "current_url": "https://example.test/pr-1/app-web/current"
                }},
                {{
                  "id": "app-admin",
                  "label": "Admin console",
                  "baseline": {admin_baseline:?},
                  "current": {admin_current:?},
                  "gallery_url": "https://example.test/pr-1/app-admin",
                  "baseline_url": "https://example.test/pr-1/app-admin/baseline",
                  "current_url": "https://example.test/pr-1/app-admin/current"
                }}
              ]
            }}"#,
            web_current = path_str(&root.join("app-web/current")),
            web_manifest = path_str(&web_manifest),
            admin_baseline = path_str(&root.join("app-admin/baseline")),
            admin_current = path_str(&root.join("app-admin/current")),
        ),
    )
    .unwrap();
    path_str(&spec)
}

#[test]
fn comment_aggregated_renders_one_comment_for_many_projects() {
    let dir = TempDir::new().unwrap();
    let spec = write_aggregate_spec(dir.path());

    let (code, out) = invoke(&["screencomp", "comment", "--projects", &spec]);
    assert_eq!(code.unwrap(), 0);

    // ONE comment, one stable aggregate marker (not a per-project marker).
    assert!(out.starts_with("<!-- screencomp-aggregate -->"), "{out}");
    assert_eq!(out.matches("<!--").count(), 1, "exactly one marker: {out}");
    // Combined summary totals every project (app-web: +1 ~1; app-admin: -1).
    assert!(
        out.contains(
            "**2 projects with visual changes · 0 projects unchanged · 1 added · 1 changed · 1 removed**"
        ),
        "{out}"
    );
    assert!(!out.contains("| Project |"), "{out}");
    let admin = out.find("### Admin console").expect("admin section");
    let web = out.find("### app-web").expect("web section");
    assert!(admin < web, "sections are label-ordered: {out}");
    assert!(
        out.contains("src=\"https://example.test/pr-1/app-web/current/pricing.png\""),
        "{out}"
    );
    assert!(
        out.contains("src=\"https://example.test/pr-1/app-admin/baseline/about.png\""),
        "{out}"
    );
}

#[test]
fn comment_aggregated_rejects_a_project_without_a_baseline() {
    let dir = TempDir::new().unwrap();
    write_capture(
        &dir.path().join("app/current"),
        &[("home", &[], &digest("aa"), "home.png", b"h")],
    );
    let spec = path_str(&dir.path().join("bad.json"));
    std::fs::write(
        &spec,
        format!(
            r#"{{"schema":2,"projects":[{{"id":"app","current":{:?}}}]}}"#,
            path_str(&dir.path().join("app/current"))
        ),
    )
    .unwrap();

    let (result, _) = invoke(&["screencomp", "comment", "--projects", &spec]);
    let err = result.unwrap_err();
    assert!(matches!(err, AppError::InvalidLayout { .. }), "{err:?}");
    assert!(err.to_string().contains("baseline"), "{err}");
}

#[test]
fn comment_aggregated_rejects_an_unknown_schema() {
    let dir = TempDir::new().unwrap();
    let spec = path_str(&dir.path().join("v9.json"));
    std::fs::write(&spec, r#"{"schema":9,"projects":[]}"#).unwrap();

    let (result, _) = invoke(&["screencomp", "comment", "--projects", &spec]);
    let err = result.unwrap_err();
    assert!(matches!(err, AppError::InvalidLayout { .. }), "{err:?}");
    assert!(err.to_string().contains("schema"), "{err}");
}

#[test]
fn comment_aggregated_rejects_schema_one_with_migration_guidance() {
    let dir = TempDir::new().unwrap();
    let spec = path_str(&dir.path().join("v1.json"));
    std::fs::write(&spec, r#"{"schema":1,"projects":[]}"#).unwrap();

    let (result, _) = invoke(&["screencomp", "comment", "--projects", &spec]);
    let err = result.unwrap_err().to_string();
    assert!(err.contains("schema 2 adds"), "{err}");
    assert!(err.contains("baseline_url"), "{err}");
    assert!(err.contains("current_url"), "{err}");
    assert!(err.contains("set `schema` to 2"), "{err}");
}

#[test]
fn comment_aggregated_rejects_an_unsafe_image_base() {
    let dir = TempDir::new().unwrap();
    write_capture(
        &dir.path().join("baseline"),
        &[("home", &[], &digest("aa"), "home.png", b"old")],
    );
    write_capture(
        &dir.path().join("current"),
        &[("home", &[], &digest("bb"), "home.png", b"new")],
    );
    let spec = path_str(&dir.path().join("unsafe.json"));
    std::fs::write(
        &spec,
        format!(
            r#"{{"schema":2,"projects":[{{"id":"app","baseline":{baseline:?},
            "current":{current:?},"baseline_url":"javascript:alert(1)",
            "current_url":"https://example.test/current"}}]}}"#,
            baseline = path_str(&dir.path().join("baseline")),
            current = path_str(&dir.path().join("current")),
        ),
    )
    .unwrap();

    let (result, _) = invoke(&["screencomp", "comment", "--projects", &spec]);
    let err = result.unwrap_err().to_string();
    assert!(err.contains("expected an absolute http(s) URL"), "{err}");
}

#[test]
fn manifest_and_classify_are_arch_scoped() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_one(&base.join("x86_64"), "home", &digest("aa"), b"v1");
    write_one(&cur.join("x86_64"), "home", &digest("bb"), b"v2");
    let manifest = path_str(&dir.path().join("x86_64.json"));

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
    // The manifest drops the arch segment (it is just the scoped index).
    let body = std::fs::read_to_string(&manifest).unwrap();
    assert!(body.contains("\"name\": \"home\""), "{body}");
    assert!(!body.contains("x86_64"), "{body}");

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
    assert!(out.contains("changed home [viewport=desktop]"), "{out}");
}

#[test]
fn malformed_manifest_is_invalid_layout_error() {
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("bad.json");
    std::fs::write(&manifest, "{not valid json").unwrap();
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
fn manifest_with_bad_digest_is_invalid_layout_error() {
    // A hand-edited index with a non-64-hex digest fails loudly rather than
    // silently dropping the shot.
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("bad.json");
    std::fs::write(
        &manifest,
        r#"{"schema":1,"shots":[{"name":"home","hash":"nothex"}]}"#,
    )
    .unwrap();
    let (result, _) = invoke(&[
        "screencomp",
        "classify",
        "--baseline-manifest",
        manifest.to_str().unwrap(),
        "--current",
        &current(),
    ]);
    let Err(AppError::InvalidLayout { reason, .. }) = result else {
        panic!("expected InvalidLayout, got {result:?}");
    };
    assert!(reason.contains("hex digest"), "{reason}");
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
fn verify_identical_captures_pass() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_one(&a, "home", &digest("aa"), b"pixels");
    write_one(&b, "home", &digest("aa"), b"pixels");

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
    // `home` differs between runs; `only_first` is dropped; `only_second` appears.
    write_capture(
        &a,
        &[
            ("home", &[], &digest("aa"), "home.png", b"v1"),
            ("only_first", &[], &digest("11"), "of.png", b"x"),
        ],
    );
    write_capture(
        &b,
        &[
            ("home", &[], &digest("bb"), "home.png", b"v2"),
            ("only_second", &[], &digest("22"), "os.png", b"y"),
        ],
    );

    let (code, out) = invoke(&[
        "screencomp",
        "verify",
        "--first",
        a.to_str().unwrap(),
        "--second",
        b.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("differs home"), "{out}");
    assert!(out.contains("only-in-first only_first"), "{out}");
    assert!(out.contains("only-in-second only_second"), "{out}");
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
    write_one(&a, "home", &digest("aa"), b"v1");
    write_one(&b, "home", &digest("bb"), b"v2");

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
        out.contains(r#"{"name":"home","toggles":{"viewport":"desktop"},"kind":"differs"}"#),
        "{out}"
    );
}

#[test]
fn verify_is_arch_scoped() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    let key = host_arch();
    write_one(&a.join(&key), "home", &digest("aa"), b"same");
    write_one(&b.join(&key), "home", &digest("aa"), b"same");
    // A foreign subtree diverges but must be invisible to the scoped run.
    write_one(&a.join("other-arch"), "home", &digest("11"), b"p");
    write_one(&b.join("other-arch"), "home", &digest("22"), b"q");

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
    write_capture(
        &input,
        &[
            ("home", &[], &digest("aa"), "home.png", b"a"),
            ("about", &[], &digest("bb"), "about.png", b"b"),
            ("pricing", &[], &digest("cc"), "pricing.png", b"c"),
        ],
    );

    let (code, out) = invoke(&["screencomp", "doctor", "--input", input.to_str().unwrap()]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.contains("arch: none (root holds the capture index)"),
        "{out}"
    );
    assert!(out.contains("inspected:"), "{out}");
    assert!(out.contains("captures.json"), "{out}");
    assert!(out.contains("names: 3"), "{out}");
    assert!(out.contains("about (1 shot)"), "{out}");
    assert!(out.contains("shots: 3"), "{out}");
    assert!(out.contains("toggles: none"), "{out}");
    assert!(out.contains("ok: capture index is well-formed"), "{out}");
}

#[test]
fn doctor_reports_observed_toggle_dimensions() {
    // The `home` name varies across two viewport values; with the dimension
    // declared, doctor lists the observed toggle and its values and stays clean.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();
    let input = dir.path().join("current");
    write_capture(
        &input,
        &[
            (
                "home",
                &[("viewport", "desktop")],
                &digest("aa"),
                "hd.png",
                b"a",
            ),
            (
                "home",
                &[("viewport", "mobile")],
                &digest("bb"),
                "hm.png",
                b"b",
            ),
        ],
    );

    let (code, out) = invoke(&[
        "screencomp",
        "--config",
        cfg.to_str().unwrap(),
        "doctor",
        "--input",
        input.to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("home (2 shots)"), "{out}");
    assert!(out.contains("toggles: 1"), "{out}");
    assert!(out.contains("viewport [desktop, mobile]"), "{out}");
    assert!(out.contains("ok: capture index is well-formed"), "{out}");
}

#[test]
fn doctor_flags_an_undeclared_toggle_as_a_problem() {
    // A toggle key with no declared `[[toggle]]` dimension cannot render a gallery
    // control, so doctor reports it as a problem and `--exit-code` gates on it.
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    write_capture(
        &input,
        &[("home", &[("density", "2x")], &digest("aa"), "h.png", b"a")],
    );

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("warning:"), "{out}");
    assert!(out.contains("density"), "{out}");
    assert!(out.contains("not declared in [[toggle]]"), "{out}");
    assert!(out.contains("problems found"), "{out}");
}

#[test]
fn doctor_flags_a_referenced_image_missing_on_disk() {
    // The index references a PNG the capture step never wrote: a silently broken
    // gallery. doctor flags the missing image and gates with `--exit-code`.
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::write(
        input.join("captures.json"),
        format!(
            r#"{{"schema":1,"shots":[{{"name":"home","toggles":{{}},"hash":"{}","image":"gone.png"}}]}}"#,
            digest("aa")
        ),
    )
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("warning:"), "{out}");
    assert!(out.contains("gone.png"), "{out}");
    assert!(out.contains("problems found"), "{out}");
}

#[test]
fn doctor_resolves_auto_arch_key() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    let key = host_arch();
    write_one(&input.join(&key), "home", &digest("aa"), b"a");

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
    write_one(&input.join(&key), "home", &digest("aa"), b"a");

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
    write_one(&input.join("x86_64"), "home", &digest("aa"), b"a");

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
    // An undeclared toggle key is a problem; the JSON carries the full contract.
    write_capture(
        &input,
        &[(
            "home",
            &[("density", "2x")],
            &digest("aa"),
            "home.png",
            b"a",
        )],
    );

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
    assert!(out.contains(r#""arch":null"#), "{out}");
    assert!(out.contains(r#"{"name":"home","shots":1}"#), "{out}");
    assert!(out.contains(r#""undeclared_toggles":["#), "{out}");
    assert!(out.contains("density"), "{out}");
    assert!(out.contains(r#""missing_images":[]"#), "{out}");
}

#[test]
fn verify_and_doctor_quiet_suppress_human_output() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("run-a");
    let b = dir.path().join("run-b");
    write_one(&a, "home", &digest("aa"), b"v1");
    write_one(&b, "home", &digest("bb"), b"v2");

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
fn doctor_exit_code_flags_an_empty_capture() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("current");
    std::fs::create_dir_all(&input).unwrap();
    // A valid but empty index: nothing downstream would render.
    std::fs::write(input.join("captures.json"), r#"{"schema":1,"shots":[]}"#).unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--input",
        input.to_str().unwrap(),
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert!(out.contains("warning:"), "{out}");
    assert!(out.contains("no screenshots"), "{out}");
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
    let changed = write_changed(dir.path(), &["shots/baseline/x.json", "Cargo.toml"]);

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
        out.contains(r#""paths":["shots/baseline/x.json"]"#),
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
    let changed = write_changed(dir.path(), &["shots/current/captures.json"]);

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
    write_one(&cur, "home", &digest("aa"), b"new-bytes");

    // Manifest holds the same shot identity but a different digest, named for an
    // arch that cannot be the host.
    let other = if host_arch() == "x86_64" {
        "arm64"
    } else {
        "x86_64"
    };
    let manifest = dir.path().join(format!("{other}.json"));
    std::fs::write(
        &manifest,
        format!(
            r#"{{"schema":1,"shots":[{{"name":"home","toggles":{{"viewport":"desktop"}},"hash":"{}"}}]}}"#,
            digest("ff")
        ),
    )
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
    // A manifest not named after an arch (baseline.json) must not trip the
    // filename heuristic, and a matching, identical shot leaves doctor clean.
    // A bare shot (no toggles) keeps the index free of undeclared-toggle problems.
    let dir = TempDir::new().unwrap();
    let cur = dir.path().join("current");
    write_capture(&cur, &[("home", &[], &digest("aa"), "home.png", b"pixels")]);

    // Generate a correct baseline from the capture itself, named non-arch-like.
    let manifest = dir.path().join("baseline.json");
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
    assert!(out.contains("ok: capture index is well-formed"), "{out}");
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
        assert!(!toml.contains("\"auto\""), "{toml}");

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
fn init_scaffolds_json_baselines() {
    // The scaffold references `.json` digest baselines (the new index shape),
    // not the old `.sha256` text manifests: the hook and the config point at
    // `shots/baseline/<arch>.json`.
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());
    invoke(&["screencomp", "init", "--dir", &root, "--arch", "arm64"])
        .0
        .unwrap();

    let hook = std::fs::read_to_string(dir.path().join(".githooks/pre-push")).unwrap();
    assert!(
        hook.contains("shots/baseline/${ARCH}.json"),
        "hook must point at a .json baseline: {hook}"
    );
    assert!(!hook.contains(".sha256"), "{hook}");
}

/// Whether `git` is installed, so git-dependent assertions skip cleanly.
fn git_available() -> bool {
    command("git").arg("--version").output().is_ok()
}

#[test]
fn doctor_env_json_flags_a_scaffolded_but_unenabled_guard() {
    let dir = TempDir::new().unwrap();
    // A committed hook with no `core.hooksPath` (a bare temp dir is not a repo):
    // the inert-guard gap, reported as a problem in the JSON contract.
    let hook = dir.path().join(".githooks/pre-push");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/usr/bin/env bash\n").unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--env",
        "--dir",
        dir.path().to_str().unwrap(),
        "--format",
        "json",
        "--exit-code",
    ]);
    assert_eq!(code.unwrap(), 3);
    assert_eq!(out.lines().count(), 1, "JSON must be one line: {out}");
    assert!(
        out.contains(r#""pre_push_guard":"present-not-enabled""#),
        "{out}"
    );
    assert!(out.contains(r#""ok":false"#), "{out}");
}

#[test]
fn doctor_env_json_flags_a_version_skew_and_reads_the_pin() {
    let dir = TempDir::new().unwrap();
    let workflows = dir.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(
        workflows.join("visual-docs.yml"),
        "uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v9.9.9\n",
    )
    .unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--env",
        "--dir",
        dir.path().to_str().unwrap(),
        "--format",
        "json",
    ]);
    // No --exit-code, so the run still exits 0 while reporting the skew.
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(r#""workflow_pin":"skew""#), "{out}");
    assert!(out.contains(r#""pinned_version":"9.9.9""#), "{out}");
    assert!(out.contains(r#""ok":false"#), "{out}");
}

#[test]
fn doctor_env_reports_a_workflow_without_a_recognizable_pin() {
    let dir = TempDir::new().unwrap();
    let workflows = dir.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    // A workflow that exists but pins nothing screencomp recognizes.
    std::fs::write(workflows.join("visual-docs.yml"), "name: something else\n").unwrap();

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--env",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains("no recognizable"), "{out}");
}

#[test]
fn doctor_env_reports_a_custom_hooks_path() {
    if !git_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    // A repo wired to a different hook manager (no committed .githooks/pre-push).
    assert!(
        command("git")
            .args(["-C", path, "init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        command("git")
            .args(["-C", path, "config", "core.hooksPath", "my-hooks"])
            .status()
            .unwrap()
            .success()
    );

    let (code, out) = invoke(&[
        "screencomp",
        "doctor",
        "--env",
        "--dir",
        path,
        "--format",
        "json",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(r#""pre_push_guard":"custom""#), "{out}");
    assert!(out.contains(r#""hooks_path":"my-hooks""#), "{out}");
    assert!(out.contains(r#""ok":true"#), "{out}");
}

#[test]
fn init_enable_hook_json_reports_enabled_in_a_repo() {
    if !git_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    assert!(
        command("git")
            .args(["-C", path, "init", "-q"])
            .status()
            .unwrap()
            .success()
    );

    let (code, out) = invoke(&[
        "screencomp",
        "init",
        "--dir",
        path,
        "--arch",
        "auto",
        "--enable-hook",
        "--format",
        "json",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(out.contains(r#""hook_enabled":"enabled""#), "{out}");
}

#[test]
fn doctor_env_quiet_suppresses_human_output() {
    let dir = TempDir::new().unwrap();
    let (code, out) = invoke(&[
        "screencomp",
        "-q",
        "doctor",
        "--env",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(code.unwrap(), 0);
    assert!(
        out.is_empty(),
        "quiet env doctor stdout should be empty: {out}"
    );
}
