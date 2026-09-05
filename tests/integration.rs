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

// The reusable workflow's embedded validation shell runs only on GitHub's Linux
// runners; this test drives that snippet through `bash`, so it is scoped to Unix.
// git-bash on the Windows CI runner rejects valid input for reasons that never
// occur in the Linux-only workflow, and the screencomp CLI itself stays fully
// covered on Windows by the other tests.
#[cfg(unix)]
#[test]
fn reusable_workflow_preserves_independent_affected_project_lanes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reusable =
        std::fs::read_to_string(root.join(".github/workflows/visual-docs-reusable.yml")).unwrap();
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();

    for contract in [
        "projects: ${{ needs.affected.outputs.projects }}",
        "SCREENCOMP_PROJECT: ${{ matrix.project }}",
        "current: ${{ matrix.current }}",
        "manifest: ${{ matrix.manifest }}",
        "project: ${{ matrix.project }}",
    ] {
        assert!(
            reusable.contains(contract),
            "affected-project workflow contract missing `{contract}`"
        );
    }
    assert!(
        reusable.contains(
            "matrix.project && format('screencomp-shots-{0}-{1}', matrix.project, matrix.arch)"
        ),
        "project artifacts must be independently addressed"
    );
    assert!(
        reusable.contains("format('screencomp-shots-{0}', matrix.arch)"),
        "the single-capture artifact name must remain backward compatible"
    );
    assert!(
        reusable.contains("path: shots") && reusable.contains("max-parallel: 1"),
        "artifact transfer must preserve shots/ roots and report writes must be serialized"
    );
    assert!(
        action.contains("screencomp-${project}${arch:+-${arch}}")
            && action.contains("subpath=\"/${project}${subpath}\"")
            && action.contains("shots/baseline/${project}/${arch}.json"),
        "composite action must isolate each project's comment, gallery, and baseline"
    );
    for unsafe_interpolation in [
        "manifest='${{ inputs.manifest }}'",
        "--title '${{ inputs.gallery-title }}'",
        "--current '${{ inputs.current }}'",
        "base_ref='${{ inputs.comment-base-ref }}'",
    ] {
        assert!(
            !action.contains(unsafe_interpolation),
            "dynamic action input remains interpolated into shell source: {unsafe_interpolation}"
        );
    }
    assert!(
        action.contains("GALLERY_TITLE: ${{ inputs.gallery-title }}")
            && action
                .contains("args=(--input \"$CURRENT\" --output site --title \"$GALLERY_TITLE\")"),
        "dynamic action fields must cross into shell through env and quoted argv"
    );

    // Execute the workflow's exact jq validation block, not a reimplementation,
    // so malformed runtime matrices fail at the same boundary the action uses.
    let validation_start = reusable
        .find("          projects=\"$PROJECTS_INPUT\"")
        .unwrap();
    let validation_end = reusable[validation_start..]
        .find("          combined=")
        .map(|offset| validation_start + offset)
        .unwrap();
    let validation = reusable[validation_start..validation_end]
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    // Feed the snippet to bash as source (`-c`), never as a filesystem path:
    // a temp-file path handed to `bash` is a Windows backslash path the runner's
    // bash cannot resolve, so the script would silently fail for every input.
    let validation_script = format!("set -euo pipefail\n{validation}");
    let dir = TempDir::new().unwrap();

    for projects in [
        r#"[{"id":"shop","current":"/tmp/shots"}]"#,
        r#"[{"id":"shop","current":"captures/shop"}]"#,
        r#"[{"id":"shop","verify":"shots/../secrets"}]"#,
        r#"[{"id":"shop","manifest":""}]"#,
        r#"[{"id":"bad/id"}]"#,
        r#"[{"id":""}]"#,
        r#"[{"id":"shop"},{"id":"shop"}]"#,
    ] {
        assert!(
            !std::process::Command::new("bash")
                .arg("-c")
                .arg(&validation_script)
                .env("PROJECTS_INPUT", projects)
                .status()
                .unwrap()
                .success(),
            "workflow accepted invalid projects: {projects}"
        );
    }
    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(&validation_script)
            .env(
                "PROJECTS_INPUT",
                r#"[{"id":"shop","current":"shots/current/shop's","manifest":"baselines/shop's/x86_64.json","gallery-title":"Shop's screenshots"}]"#,
            )
            .status()
            .unwrap()
            .success(),
        "workflow rejected a valid affected project"
    );

    // upload-artifact stores the contents of `path: shots`; download-artifact
    // restores those contents at `path: shots`. Exercise that boundary with a
    // non-default per-project root and verify report sees the same file.
    let capture = dir.path().join("capture");
    let report = dir.path().join("report");
    let custom = capture.join("shots/custom/shop/x86_64");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(custom.join("captures.json"), b"{\"schema\":1,\"shots\":[]}").unwrap();
    // Mirror upload-artifact(path: shots) -> download-artifact(path: shots) with a
    // cross-platform recursive copy; a `cp` shell-out is not on the Windows PATH
    // and its `\`-separated destination is not a POSIX path.
    copy_tree(&capture.join("shots"), &report.join("shots"));
    assert_eq!(
        std::fs::read(report.join("shots/custom/shop/x86_64/captures.json")).unwrap(),
        b"{\"schema\":1,\"shots\":[]}"
    );

    // Execute the composite action's exact gallery shell with hostile apostrophes.
    // Values arrive through env and remain single argv values instead of becoming
    // shell source.
    let gallery_step = action.find("    - name: Build gallery").unwrap();
    let gallery_run = action[gallery_step..].find("      run: |\n").unwrap()
        + gallery_step
        + "      run: |\n".len();
    let gallery_end = action[gallery_run..]
        .find("\n    - name:")
        .map(|offset| gallery_run + offset)
        .unwrap();
    let gallery_script = action[gallery_run..gallery_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let calls = dir.path().join("gallery-args");
    let executable = format!(
        "#!/usr/bin/env bash\nscreencomp() {{ printf '%s\\n' \"$@\" >\"$CALLS\"; }}\n{gallery_script}"
    );
    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(&executable)
            .env("CALLS", &calls)
            .env("CURRENT", "shots/current/shop's")
            .env("ARCH", "x86_64")
            .env("GALLERY_TITLE", "Shop's screenshots")
            .env("BASELINE_FOUND", "")
            .env("BASELINE_PATH", "")
            .status()
            .unwrap()
            .success()
    );
    let args = std::fs::read_to_string(&calls).unwrap();
    assert!(args.lines().any(|arg| arg == "shots/current/shop's"));
    assert!(args.lines().any(|arg| arg == "Shop's screenshots"));
    assert!(!args.lines().any(|arg| arg == "--baseline"));

    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(&executable)
            .env("CALLS", &calls)
            .env("CURRENT", "shots/current/shop's")
            .env("ARCH", "x86_64")
            .env("GALLERY_TITLE", "Shop's screenshots")
            .env("BASELINE_FOUND", "true")
            .env("BASELINE_PATH", "deployed/shop's")
            .status()
            .unwrap()
            .success()
    );
    let args = std::fs::read_to_string(calls).unwrap();
    assert!(args.lines().any(|arg| arg == "--baseline"));
    assert!(args.lines().any(|arg| arg == "deployed/shop's"));
    assert!(args.lines().any(|arg| arg == "--focused"));
}

#[cfg(unix)]
#[test]
fn visual_docs_external_pages_contract_and_preview_fallback_are_wired() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reusable =
        std::fs::read_to_string(root.join(".github/workflows/visual-docs-reusable.yml")).unwrap();
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let aggregate = std::fs::read_to_string(root.join("visual-docs-aggregate/action.yml")).unwrap();
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();

    for contract in [
        "\n  pages-repository:",
        "\n  pages-token:",
        "external_repository: ${{ inputs.pages-repository }}",
        "personal_token: ${{ inputs.pages-token }}",
        "args+=(--baseline \"$BASELINE_PATH\" --focused)",
        "repo: ${{ inputs.pages-repository || github.repository }}",
    ] {
        assert!(
            action.contains(contract)
                || reusable.contains(contract)
                || aggregate.contains(contract)
                || readme.contains(contract),
            "{contract}"
        );
    }
    assert!(
        reusable.contains("pages-repository: ${{ inputs.pages-repository }}")
            && reusable.contains("pages-token: ${{ secrets.pages-token }}"),
        "reusable workflow must forward external Pages credentials"
    );
    assert!(
        aggregate.contains("pages_repo=\"${OWNER}/${REPO_NAME}\"")
            && aggregate
                .contains("main_url=\"https://${pages_repo%%/*}.github.io/${pages_repo#*/}\"")
            && aggregate.contains("pages-repository must be an owner/name"),
        "aggregated comments must validate and derive the same external Pages host"
    );
    assert!(
        readme.contains("pages-repository: your-org/visual-docs-pages")
            && readme.contains("pages-token: ${{ secrets.VISUAL_DOCS_PAGES_TOKEN }}")
            && readme.contains("must be public"),
        "external Pages documentation must stay aligned with the action contract"
    );

    let config_step = action.find("    - name: Resolve config").unwrap();
    let config_run = action[config_step..].find("      run: |\n").unwrap()
        + config_step
        + "      run: |\n".len();
    let config_end = action[config_run..]
        .find("\n    - name:")
        .map(|offset| config_run + offset)
        .unwrap();
    let config_script = action[config_run..config_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("${{ github.repository }}", "source/app")
        .replace("${{ github.repository_owner }}", "source")
        .replace("${{ github.event.repository.name }}", "app")
        .replace("${{ github.event.pull_request.number }}", "17");
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("output");
    let base_env = |command: &mut std::process::Command| {
        command
            .env("INPUT_ARCH", "arm64")
            .env("INPUT_PROJECT", "web")
            .env("INPUT_MANIFEST", "")
            .env("INPUT_GALLERY_URL", "")
            .env("INPUT_BASELINE_URL", "")
            .env("INPUT_PAGES", "true")
            .env("INPUT_PUBLISH", "true")
            .env("GITHUB_OUTPUT", &output);
    };

    let mut missing = std::process::Command::new("bash");
    missing.arg("-c").arg(&config_script);
    base_env(&mut missing);
    let failure = missing
        .env("INPUT_PAGES_REPOSITORY", "docs/galleries")
        .env("INPUT_PAGES_TOKEN", "")
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(
        String::from_utf8_lossy(&failure.stderr).contains("pages-token is required"),
        "{}",
        String::from_utf8_lossy(&failure.stderr)
    );
    let mut invalid = std::process::Command::new("bash");
    invalid.arg("-c").arg(&config_script);
    base_env(&mut invalid);
    let failure = invalid
        .env("INPUT_PAGES_REPOSITORY", "not-a-repository")
        .env("INPUT_PAGES_TOKEN", "token")
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(
        String::from_utf8_lossy(&failure.stderr).contains("pages-repository must be an owner/name")
    );

    let mut external = std::process::Command::new("bash");
    external.arg("-c").arg(&config_script);
    base_env(&mut external);
    let success = external
        .env("INPUT_PAGES_REPOSITORY", "docs/galleries")
        .env("INPUT_PAGES_TOKEN", "token")
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let outputs = std::fs::read_to_string(&output).unwrap();
    assert!(
        outputs.contains("gallery_url=https://docs.github.io/galleries/pr-17/web/arm64"),
        "{outputs}"
    );
    assert!(
        outputs.contains("baseline_url=https://docs.github.io/galleries/web/arm64"),
        "{outputs}"
    );

    // Execute the action's canonical-baseline fetch against a real local
    // gh-pages branch. Only the remote URL is replaced; sparse checkout,
    // branch fetch, index detection, and output resolution are the shipped shell.
    let remote = dir.path().join("pages");
    std::fs::create_dir_all(remote.join("web/arm64")).unwrap();
    std::fs::write(
        remote.join("web/arm64/captures.json"),
        r#"{"schema":1,"shots":[]}"#,
    )
    .unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.com"],
        vec!["add", "."],
        vec!["commit", "-qm", "gallery"],
        vec!["branch", "-M", "gh-pages"],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&remote)
                .status()
                .unwrap()
                .success()
        );
    }
    let fetch_step = action
        .find("    - name: Fetch canonical gallery baseline")
        .unwrap();
    let fetch_run =
        action[fetch_step..].find("      run: |\n").unwrap() + fetch_step + "      run: |\n".len();
    let fetch_end = action[fetch_run..]
        .find("\n    - name:")
        .map(|offset| fetch_run + offset)
        .unwrap();
    let fetch_script = action[fetch_run..fetch_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
        .replace(
            "\"https://github.com/${PAGES_REPO}.git\"",
            &format!("\"{}\"", remote.display()),
        );
    let fetch_output = dir.path().join("fetch-output");
    let fetch = std::process::Command::new("bash")
        .arg("-c")
        .arg(&fetch_script)
        .env("PAGES_REPO", "docs/galleries")
        .env("PAGES_TOKEN", "token")
        .env("DEST", "web/arm64")
        .env("ARCH", "arm64")
        .env("RUNNER_TEMP", dir.path())
        .env("GITHUB_OUTPUT", &fetch_output)
        .output()
        .unwrap();
    assert!(
        fetch.status.success(),
        "{}",
        String::from_utf8_lossy(&fetch.stderr)
    );
    let fetch_outputs = std::fs::read_to_string(&fetch_output).unwrap();
    assert!(fetch_outputs.contains("found=true"), "{fetch_outputs}");
    let baseline_root = fetch_outputs
        .lines()
        .find_map(|line| line.strip_prefix("path="))
        .unwrap();
    assert!(
        Path::new(baseline_root)
            .join("arm64/captures.json")
            .is_file(),
        "{fetch_outputs}"
    );

    let missing_output = dir.path().join("missing-output");
    let missing_index = std::process::Command::new("bash")
        .arg("-c")
        .arg(&fetch_script)
        .env("PAGES_REPO", "docs/galleries")
        .env("PAGES_TOKEN", "token")
        .env("DEST", "missing/arm64")
        .env("ARCH", "arm64")
        .env("RUNNER_TEMP", dir.path())
        .env("GITHUB_OUTPUT", &missing_output)
        .output()
        .unwrap();
    assert!(missing_index.status.success());
    assert_eq!(
        std::fs::read_to_string(&missing_output).unwrap(),
        "found=false\n"
    );
    assert!(
        String::from_utf8_lossy(&missing_index.stdout)
            .contains("no canonical gallery at missing/arm64")
    );

    let no_branch = dir.path().join("pages-without-gh-pages");
    std::fs::create_dir_all(&no_branch).unwrap();
    std::fs::write(no_branch.join("README"), "seed").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.com"],
        vec!["add", "."],
        vec!["commit", "-qm", "seed"],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&no_branch)
                .status()
                .unwrap()
                .success()
        );
    }
    let no_branch_script = fetch_script.replace(
        &remote.display().to_string(),
        &no_branch.display().to_string(),
    );
    let no_branch_output = dir.path().join("no-branch-output");
    let branch_absent = std::process::Command::new("bash")
        .arg("-c")
        .arg(no_branch_script)
        .env("PAGES_REPO", "docs/galleries")
        .env("PAGES_TOKEN", "token")
        .env("DEST", "web/arm64")
        .env("ARCH", "arm64")
        .env("RUNNER_TEMP", dir.path())
        .env("GITHUB_OUTPUT", &no_branch_output)
        .output()
        .unwrap();
    assert!(branch_absent.status.success());
    assert_eq!(
        std::fs::read_to_string(&no_branch_output).unwrap(),
        "found=false\n"
    );
    assert!(
        String::from_utf8_lossy(&branch_absent.stdout).contains("no canonical gallery branch yet")
    );

    // One first-job preflight gates matrix resolution and every side-effecting
    // path, including event-only maintenance jobs.
    for dependency in [
        "arches:\n    needs: pages-preflight",
        "needs: [pages-preflight, arches]",
        "needs: [pages-preflight, arches, capture]",
        "cleanup-preview:\n    needs: pages-preflight",
        "prune-history:\n    needs: pages-preflight",
    ] {
        assert!(reusable.contains(dependency), "{dependency}");
    }
    let validation_marker = "      - name: Validate external Pages configuration";
    assert_eq!(reusable.matches(validation_marker).count(), 1);
    let block = &reusable[reusable.find(validation_marker).unwrap()..];
    let run_start = block.find("        run: |\n").unwrap() + "        run: |\n".len();
    let run_end = block[run_start..].find("\n\n  #").unwrap() + run_start;
    let script = block[run_start..run_end]
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    for (repo, token, succeeds) in [
        ("", "", true),
        ("docs/galleries", "token", true),
        ("docs/galleries", "", false),
        ("invalid", "token", false),
    ] {
        let result = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env("PAGES_REPOSITORY", repo)
            .env("PAGES_TOKEN", token)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            succeeds,
            "repo={repo:?}, stderr={}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let aggregate_step = aggregate
        .find("    - name: Render and upsert the aggregated comment")
        .unwrap();
    let aggregate_run = aggregate[aggregate_step..].find("      run: |\n").unwrap()
        + aggregate_step
        + "      run: |\n".len();
    let aggregate_end = aggregate[aggregate_run..]
        .find("\n    - name:")
        .map_or(aggregate.len(), |offset| aggregate_run + offset);
    let aggregate_script = aggregate[aggregate_run..aggregate_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let invalid_aggregate = std::process::Command::new("bash")
        .arg("-c")
        .arg(aggregate_script)
        .env("COMMENT_BASE_REF", "")
        .env("BASE_REF_DEFAULT", "")
        .env("PAGES_REPOSITORY", "invalid")
        .output()
        .unwrap();
    assert!(!invalid_aggregate.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_aggregate.stderr)
            .contains("pages-repository must be an owner/name")
    );

    let justfile = std::fs::read_to_string(root.join("justfile")).unwrap();
    assert!(
        justfile.contains("\ngate: check\n"),
        "`just gate` must remain an alias of the full check gate"
    );
}

#[test]
fn aggregated_comment_mode_is_wired_end_to_end() {
    // The aggregated surface spans three files that must stay in lockstep: the
    // reusable workflow exposes `comment-mode` and forwards it, the per-project
    // action suppresses its own comment in that mode, and the `visual-docs-aggregate`
    // action composes `screencomp comment --projects` into one upserted comment.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reusable =
        std::fs::read_to_string(root.join(".github/workflows/visual-docs-reusable.yml")).unwrap();
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let aggregate = std::fs::read_to_string(root.join("visual-docs-aggregate/action.yml")).unwrap();

    // Reusable workflow: the mode is a real input, forwarded to the per-project
    // action, and there is an aggregate-comment job composing the aggregate action.
    assert!(
        reusable.contains("\n      comment-mode:"),
        "reusable workflow missing comment-mode input"
    );
    assert!(
        reusable.contains("comment-mode: ${{ inputs.comment-mode }}"),
        "report job must forward comment-mode to the per-project action"
    );
    assert!(
        reusable.contains("aggregate-comment:")
            && reusable.contains("inputs.comment-mode == 'aggregated'")
            && reusable.contains("uses: nickderobertis/screencomp/visual-docs-aggregate@v0"),
        "aggregate-comment job must gate on the mode and compose the aggregate action"
    );
    // The aggregate job hands the resolved matrix to the action.
    assert!(
        reusable.contains("matrix: ${{ toJSON(fromJSON(needs.arches.outputs.matrix).include) }}"),
        "aggregate-comment must pass the resolved capture matrix"
    );

    // Per-project action: a comment-mode input whose 'aggregated' value suppresses
    // this lane's own comment (so it isn't double-posted alongside the combined one).
    assert!(
        action.contains("comment-mode:"),
        "per-project action missing comment-mode input"
    );
    assert!(
        action.contains("inputs.comment-mode != 'aggregated'"),
        "per-project comment steps must be suppressed in aggregated mode"
    );

    // Aggregate action: builds a schema-2 projects spec and renders it with a
    // single stable aggregate marker, upserting by that marker.
    assert!(
        aggregate.contains("screencomp comment --projects")
            && aggregate.contains("{schema: 2, projects: $projects}")
            && aggregate.contains("baseline_url: $baseline_url")
            && aggregate.contains("current_url: $current_url")
            && aggregate.contains("marker=\"screencomp-aggregate\""),
        "aggregate action must compose `comment --projects` under a stable marker"
    );
}

/// Recursively copy `src` into `dst` (creating `dst`), using only path APIs so
/// the artifact round-trip behaves identically on Windows and Unix.
// Only used by the Unix-scoped reusable-workflow lanes test above.
#[cfg(unix)]
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Whether `git` is installed, so git-dependent assertions skip cleanly.
fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
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
        std::process::Command::new("git")
            .args(["-C", path, "init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
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
        std::process::Command::new("git")
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

/// Extract one composite-action step's `run:` as a runnable bash script,
/// undenting it and substituting the `github.*` expressions the runner would
/// have expanded. The shipped shell is then executed verbatim. Both a `run: |`
/// block and a one-line `run:` are supported.
#[cfg(unix)]
fn action_step_script(action: &str, step_name: &str) -> String {
    let start = action
        .find(&format!("    - name: {step_name}\n"))
        .unwrap_or_else(|| panic!("no step named {step_name}"));
    // Bound the slice to this step first: a step whose `run:` is a single line
    // would otherwise pick up a later step's block.
    let step = &action[start..];
    let step = step
        .split_once("\n    - name:")
        .map_or(step, |(head, _)| head);
    let body = match step.split_once("      run: |\n") {
        Some((_, block)) => block
            .lines()
            .map(|line| line.strip_prefix("        ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n"),
        None => step
            .split_once("      run: ")
            .unwrap_or_else(|| panic!("step {step_name} has no run:"))
            .1
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
    };
    body.replace("${{ github.repository }}", "source/app")
        .replace("${{ github.repository_owner }}", "source")
        .replace("${{ github.event.repository.name }}", "app")
        .replace("${{ github.event.pull_request.number }}", "17")
}

/// Run the `visual-docs` action's "Resolve config" step for one project/arch lane
/// and return its `$GITHUB_OUTPUT` key/value lines.
#[cfg(unix)]
fn resolve_lane_config(action: &str, dir: &Path, lane: &str, project: &str, arch: &str) -> String {
    let output = dir.join(format!("cfg-{lane}"));
    let result = std::process::Command::new("bash")
        .arg("-c")
        .arg(action_step_script(action, "Resolve config"))
        .env("INPUT_ARCH", arch)
        .env("INPUT_PROJECT", project)
        .env("INPUT_MANIFEST", "")
        .env("INPUT_GALLERY_URL", "")
        .env("INPUT_BASELINE_URL", "")
        .env("INPUT_PAGES", "true")
        .env("INPUT_PUBLISH", "true")
        .env("INPUT_PAGES_REPOSITORY", "")
        .env("INPUT_PAGES_TOKEN", "")
        .env("GITHUB_OUTPUT", &output)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read_to_string(&output).unwrap()
}

#[cfg(unix)]
fn output_value(outputs: &str, key: &str) -> String {
    outputs
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_default()
        .to_string()
}

/// Every report lane hands its gallery off staged under the exact subpath it
/// would otherwise have pushed to, so merging the artifacts reconstructs the tree
/// N per-lane pushes produced — the property that lets ONE commit replace N and
/// take the superseded-Pages-build race with it.
#[cfg(unix)]
#[test]
fn coalesced_pages_deploy_merges_every_lane_into_one_publishable_tree() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let stage = action_step_script(&action, "Stage the gallery for a coalesced deploy");
    let dir = TempDir::new().unwrap();

    // Two projects on one arch, plus the project-level layout that deploys to the
    // branch root — the three destination shapes the action can produce.
    let lanes = [
        ("web", "web", "arm64"),
        ("shop", "shop", "arm64"),
        ("plain", "", ""),
    ];
    let merged = dir.path().join("merged");
    std::fs::create_dir_all(&merged).unwrap();

    for event in ["pull_request", "push"] {
        for (lane, project, arch) in lanes {
            let work = dir.path().join(format!("{event}-{lane}"));
            std::fs::create_dir_all(work.join("site/img")).unwrap();
            std::fs::write(work.join("site/index.html"), format!("gallery {lane}")).unwrap();
            std::fs::write(work.join("site/img/home.png"), b"png").unwrap();

            let outputs = resolve_lane_config(&action, dir.path(), lane, project, arch);
            let staged = std::process::Command::new("bash")
                .arg("-c")
                .arg(&stage)
                .env("DEST", output_value(&outputs, "dest"))
                .env("SUBPATH", output_value(&outputs, "subpath"))
                .env("EVENT_NAME", event)
                .env("PR_NUMBER", "17")
                .current_dir(&work)
                .output()
                .unwrap();
            assert!(
                staged.status.success(),
                "{}",
                String::from_utf8_lossy(&staged.stderr)
            );

            // `actions/download-artifact` with merge-multiple unpacks every lane's
            // upload into one directory; copying them over each other is that.
            let unpack = std::process::Command::new("bash")
                .arg("-c")
                .arg(format!(
                    "cp -R {}/. {}/",
                    work.join("pages-upload").display(),
                    merged.display()
                ))
                .output()
                .unwrap();
            assert!(unpack.status.success());
        }
    }

    // The PR event nests every lane under this PR's preview prefix; the push event
    // publishes the canonical paths. Both land in the same tree, so one root push
    // with keep_files deploys exactly what the per-lane pushes would have.
    for path in [
        "pr-17/web/arm64/index.html",
        "pr-17/web/arm64/img/home.png",
        "pr-17/shop/arm64/index.html",
        "pr-17/index.html",
        "web/arm64/index.html",
        "shop/arm64/index.html",
        "index.html",
    ] {
        assert!(merged.join(path).is_file(), "missing {path}");
    }
    assert_eq!(
        std::fs::read_to_string(merged.join("pr-17/shop/arm64/index.html")).unwrap(),
        "gallery shop",
        "each lane's gallery must survive the merge intact"
    );
}

/// Write a stub `gh` that replaces only the GitHub API boundary: it answers each
/// `--jq` selector from a scripted list of "<build-id> <status>" polls and
/// records rebuild requests, so the shipped script's decisions — real bash, real
/// script — are the only thing under test. `$WORK` must point at `work`.
#[cfg(unix)]
fn write_gh_stub(work: &Path, polls: &[&str]) -> PathBuf {
    std::fs::create_dir_all(work).unwrap();
    let stub = work.join("gh");
    std::fs::write(
        &stub,
        r#"#!/usr/bin/env bash
set -uo pipefail
filter=""; method=""; prev=""
for arg in "$@"; do
  case "$prev" in --jq) filter="$arg" ;; --method) method="$arg" ;; esac
  prev="$arg"
done
if [ "$method" = POST ]; then
  echo rebuild >>"$WORK/posts"
  [ ! -f "$WORK/deny-rebuild" ] || exit 1
  exit 0
fi
seen=$(cat "$WORK/cursor" 2>/dev/null || echo 0)
poll=$(sed -n "$((seen + 1))p" "$WORK/polls")
[ -n "$poll" ] || poll=$(tail -1 "$WORK/polls")
[ "$poll" != unreadable ] || exit 1
case "$filter" in
  # `.url` and `.status` are read as one logical poll; only the second advances.
  .url) printf 'https://api.github.com/repos/o/r/pages/builds/%s\n' "${poll%% *}" ;;
  .status) echo $((seen + 1)) >"$WORK/cursor"; printf '%s\n' "${poll##* }" ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(work.join("polls"), format!("{}\n", polls.join("\n"))).unwrap();
    stub
}

/// Count the rebuild requests the stub recorded for one work directory.
#[cfg(unix)]
fn gh_stub_rebuilds(work: &Path) -> usize {
    std::fs::read_to_string(work.join("posts"))
        .map(|log| log.lines().count())
        .unwrap_or(0)
}

/// Run one subcommand of the shipped Pages build gate against that stub.
#[cfg(unix)]
fn run_pages_build_gate(
    dir: &Path,
    label: &str,
    polls: &[&str],
    previous_build: &str,
    subcommand: &str,
) -> (std::process::Output, usize) {
    let work = dir.join(label);
    let stub = write_gh_stub(&work, polls);
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/visual-docs-pages-build.sh");
    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg(subcommand)
        .env("WORK", &work)
        .env("GH_BIN", &stub)
        .env("REPO", "o/r")
        .env("PREVIOUS_BUILD", previous_build)
        .env("POLL_SECONDS", "0")
        .env("APPEAR_ATTEMPTS", "3")
        .env("SETTLE_ATTEMPTS", "3")
        .output()
        .unwrap();
    let posts = gh_stub_rebuilds(&work);
    (output, posts)
}

/// A multi-project run must not finish green with the gallery unpublished. The
/// gate passes only once the build the deploy triggered reaches `built`, retries
/// a superseded one exactly once (the failure mode this change exists for), and
/// fails loudly when it still errors.
#[cfg(unix)]
#[test]
fn pages_build_gate_passes_on_a_built_build_and_fails_on_an_errored_one() {
    let dir = TempDir::new().unwrap();

    // `record` names the build already published, so the gate can tell the one
    // the deploy triggers apart from it.
    let (recorded, _) = run_pages_build_gate(dir.path(), "record", &["100 built"], "", "record");
    assert!(recorded.status.success());
    assert_eq!(String::from_utf8_lossy(&recorded.stdout).trim(), "100");

    // Happy path: a new build appears, finishes, and the run proceeds.
    let (ok, posts) = run_pages_build_gate(
        dir.path(),
        "built",
        &["101 building", "101 built"],
        "100",
        "verify",
    );
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("pages build succeeded"));
    assert_eq!(posts, 0, "a healthy build needs no rebuild");

    // Superseded by an external writer: `errored` with the same commit, which
    // rebuilds cleanly. Recovered, not failed — and rebuilt exactly once.
    let (recovered, posts) = run_pages_build_gate(
        dir.path(),
        "superseded",
        &["101 errored", "101 errored", "102 building", "102 built"],
        "100",
        "verify",
    );
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert!(
        String::from_utf8_lossy(&recovered.stdout).contains("succeeded for o/r after a rebuild")
    );
    assert_eq!(posts, 1);

    // Genuinely broken: still errored after the rebuild, so the run goes red
    // instead of leaving the site errored and the gallery stale.
    let (failed, posts) = run_pages_build_gate(
        dir.path(),
        "errored",
        &["101 errored", "101 errored", "102 errored"],
        "100",
        "verify",
    );
    assert!(
        !failed.status.success(),
        "an errored build must fail the run"
    );
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        stderr.contains("::error::") && stderr.contains("the published gallery is stale"),
        "{stderr}"
    );
    assert_eq!(posts, 1);

    // A token without pages:read cannot observe the build. The coalesced deploy
    // still happened, so warn rather than failing every run of a caller that
    // never granted the permission.
    let (unreadable, posts) =
        run_pages_build_gate(dir.path(), "unreadable", &["unreadable"], "", "verify");
    assert!(
        unreadable.status.success(),
        "{}",
        String::from_utf8_lossy(&unreadable.stderr)
    );
    assert!(
        String::from_utf8_lossy(&unreadable.stderr).contains("::warning::"),
        "{}",
        String::from_utf8_lossy(&unreadable.stderr)
    );
    assert_eq!(posts, 0);

    // Pages is readable but the branch drives no build (an Actions-sourced site,
    // say), so the deploy simply goes unverified. Warn with the setting to change.
    let (absent, posts) =
        run_pages_build_gate(dir.path(), "absent", &["100 built"], "100", "verify");
    assert!(
        absent.status.success(),
        "{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    assert!(
        String::from_utf8_lossy(&absent.stderr).contains("Deploy from a branch"),
        "{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    assert_eq!(posts, 0);

    // A build that never settles leaves the gallery stale just as surely as one
    // that errors, so it fails rather than timing out into a green run.
    let (stuck, _) = run_pages_build_gate(dir.path(), "stuck", &["101 building"], "100", "verify");
    assert!(!stuck.status.success());
    assert!(
        String::from_utf8_lossy(&stuck.stderr).contains("still running after 3 polls"),
        "{}",
        String::from_utf8_lossy(&stuck.stderr)
    );

    // Recovering from a supersede needs pages:write. When the rebuild is refused
    // the gate cannot recover, so it fails and names the missing permission.
    std::fs::create_dir_all(dir.path().join("denied")).unwrap();
    std::fs::write(dir.path().join("denied/deny-rebuild"), "").unwrap();
    let (denied, posts) = run_pages_build_gate(
        dir.path(),
        "denied",
        &["101 errored", "101 errored"],
        "100",
        "verify",
    );
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("pages:write"),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    assert_eq!(posts, 1);

    // A typo in the composing action must not look like a healthy deploy.
    let (unknown, _) = run_pages_build_gate(dir.path(), "unknown", &["100 built"], "", "publish");
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("want record|verify"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );

    // The repository and the poll budget reach the API path, arithmetic, and
    // `sleep`, so a malformed one is rejected up front instead of hanging or
    // producing a nonsense request.
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/visual-docs-pages-build.sh");
    for (repo, attempts, expected) in [
        ("not-a-repository", "3", "REPO must be an owner/name"),
        ("o/r", "many", "APPEAR_ATTEMPTS must be a non-negative"),
    ] {
        let rejected = std::process::Command::new("bash")
            .arg(&script)
            .arg("verify")
            .env("REPO", repo)
            .env("APPEAR_ATTEMPTS", attempts)
            .output()
            .unwrap();
        assert!(!rejected.status.success(), "{repo} {attempts}");
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
}

/// The coalescing has to be wired end to end to be worth anything: report lanes
/// must hand galleries off instead of pushing, exactly one job must push them,
/// and that job must be able to observe the resulting Pages build.
#[test]
fn coalesced_pages_deploy_is_wired_through_the_reusable_workflow() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reusable =
        std::fs::read_to_string(root.join(".github/workflows/visual-docs-reusable.yml")).unwrap();
    let report = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let deploy = std::fs::read_to_string(root.join("visual-docs-pages/action.yml")).unwrap();
    let scaffold = {
        let dir = TempDir::new().unwrap();
        let (result, _) = invoke(&["screencomp", "init", "--dir", &path_str(dir.path())]);
        assert_eq!(result.unwrap(), 0);
        std::fs::read_to_string(dir.path().join(".github/workflows/visual-docs.yml")).unwrap()
    };

    // Each lane hands off under a name the deploy job's default pattern matches.
    assert!(
        reusable.contains("pages-artifact: ${{ matrix.project && format('screencomp-gallery-{0}-{1}', matrix.project, matrix.arch) || format('screencomp-gallery-{0}', matrix.arch) }}"),
        "report lanes must hand their gallery off instead of pushing it"
    );
    assert!(
        deploy.contains("default: screencomp-gallery-*"),
        "the deploy action must collect the artifacts the lanes hand off"
    );

    // Nothing else may push: every per-lane deploy is gated on hand-off being off,
    // which is also what keeps a caller composing `visual-docs` alone unaffected.
    let per_lane_pushes = report
        .match_indices("uses: peaceiris/actions-gh-pages@v4")
        .count();
    assert_eq!(per_lane_pushes, 4, "the four per-lane deploy steps");
    assert_eq!(
        report.match_indices("inputs.pages-artifact == ''").count(),
        7,
        "each per-lane deploy, its build gate, and the preview wait must be gated on direct-deploy mode"
    );

    // One push for the whole run, at the branch root: the merged artifacts already
    // carry each lane's subpath, so a destination_dir would nest them twice.
    assert_eq!(
        deploy
            .match_indices("uses: peaceiris/actions-gh-pages@v4")
            .count(),
        2,
        "same-repository and external hosting, one push each"
    );
    assert!(
        !deploy.contains("\n        destination_dir:"),
        "the coalesced push must publish at the branch root"
    );
    assert_eq!(deploy.match_indices("keep_files: true").count(), 2);

    // The job runs even when a lane failed the strict drift gate — the gallery and
    // the comment a reviewer needs must still publish.
    assert!(
        reusable.contains(
            "if: ${{ !cancelled() && inputs.pages && inputs.publish && needs.report.result != 'skipped' }}"
        ),
        "a drifted lane must still get its gallery published"
    );
    assert!(
        reusable.contains("needs: [pages-preflight, arches, report]")
            && reusable.contains("uses: nickderobertis/screencomp/visual-docs-pages@v0"),
        "the deploy job must run after every report lane"
    );

    // Observing the build needs pages:read, which only the CALLER can grant. A
    // called job that declares a permission the caller withheld fails the whole
    // run at parse time, so the deploy job must declare none and inherit — else
    // every existing caller breaks on upgrade — while the scaffold grants it.
    let deploy_job = reusable.split("  deploy-pages:").nth(1).unwrap();
    let deploy_job = deploy_job
        .split_once("    steps:")
        .expect("the deploy job must have steps")
        .0;
    assert!(
        !deploy_job
            .lines()
            .any(|line| line.trim_start().starts_with("permissions:")),
        "declaring permissions on the deploy job breaks callers that granted less: {deploy_job}"
    );
    assert!(
        scaffold.contains("pages: read"),
        "the scaffolded caller must grant pages:read: {scaffold}"
    );

    // External hosting keeps working on the one token it already had.
    assert!(
        reusable.contains("pages-repository: ${{ inputs.pages-repository }}")
            && reusable.contains("pages-token: ${{ secrets.pages-token }}")
            && deploy.contains("personal_token: ${{ inputs.pages-token }}")
            && deploy.contains("external_repository: ${{ inputs.pages-repository }}")
    );
    assert!(
        deploy.contains("visual-docs-pages-build.sh\" record")
            && deploy.contains("visual-docs-pages-build.sh\" verify"),
        "the deploy must be gated on the Pages build it triggers"
    );

    // peaceiris pushes no commit when the published bytes are unchanged, so no
    // build starts. Gating on the branch head moving keeps a healthy no-op re-run
    // from waiting for a build that never comes and then blaming the Pages source.
    assert!(
        deploy.contains("if: ${{ steps.published.outputs.published == 'true' }}"),
        "the gate must only run when the deploy actually published a commit"
    );
    assert_eq!(
        deploy.match_indices("refs/heads/${BRANCH}").count(),
        2,
        "the head is read before and after the push, on the branch peaceiris wrote"
    );
    assert!(
        deploy.contains("publish-branch must be a plain branch name"),
        "publish-branch reaches a refs/heads/ lookup, so validate it at the boundary"
    );
}

/// Execute the coalesced deploy's no-op detection against a REAL local git
/// remote. Only the remote URL is substituted; the ref lookup and the
/// published/not-published decision are the shipped shell.
///
/// This is the step that keeps a healthy re-run of an unchanged gallery from
/// waiting out the gate's budget and then blaming the caller's Pages source:
/// peaceiris pushes no commit when the bytes match, so no build starts.
#[cfg(unix)]
#[test]
fn coalesced_deploy_detects_a_push_that_published_nothing() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let action = std::fs::read_to_string(root.join("visual-docs-pages/action.yml")).unwrap();
    let dir = TempDir::new().unwrap();
    let remote = dir.path().join("gallery");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::write(remote.join("index.html"), "gallery").unwrap();

    let git = |args: Vec<&str>| {
        assert!(
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&remote)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    };
    git(vec!["init", "-q"]);
    git(vec!["config", "user.name", "Test"]);
    git(vec!["config", "user.email", "test@example.com"]);
    git(vec!["add", "."]);
    git(vec!["commit", "-qm", "gallery"]);
    git(vec!["branch", "-M", "gh-pages"]);

    let script = action_step_script(&action, "Check whether anything was published").replace(
        "\"https://x-access-token:${GH_TOKEN}@github.com/${REPO}.git\"",
        &format!("\"{}\"", remote.display()),
    );
    let run = |before: &str, label: &str| {
        let output_file = dir.path().join(format!("out-{label}"));
        let result = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env("REPO", "docs/galleries")
            .env("BRANCH", "gh-pages")
            .env("BEFORE", before)
            .env("GH_TOKEN", "token")
            .env("GITHUB_OUTPUT", &output_file)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        (
            std::fs::read_to_string(&output_file).unwrap(),
            String::from_utf8(result.stdout).unwrap(),
        )
    };

    // The push moved nothing: no commit, so no Pages build, so nothing to gate.
    let (outputs, stdout) = run(&head_of(&remote), "unchanged");
    assert!(outputs.contains("published=false"), "{outputs}");
    assert!(
        stdout.contains("no commit, no build to gate on"),
        "{stdout}"
    );

    // A real deploy moves the branch head, and only then is there a build to wait
    // for.
    let stale = head_of(&remote);
    std::fs::write(remote.join("index.html"), "new gallery").unwrap();
    git(vec!["add", "."]);
    git(vec!["commit", "-qm", "deploy"]);
    let (outputs, stdout) = run(&stale, "published");
    assert!(outputs.contains("published=true"), "{outputs}");
    assert!(stdout.contains("published gh-pages"), "{stdout}");

    // A branch that does not exist yet (the very first deploy) reads as empty,
    // which must still count as published rather than silently skipping the gate.
    let (outputs, _) = run("", "first-deploy");
    assert!(outputs.contains("published=true"), "{outputs}");
}

/// A caller composing `visual-docs` on its own still deploys per lane, and that
/// path pushed and returned without ever observing the Pages build it started —
/// the original defect, on the route the coalesced deploy does not take. Gating it
/// through the SAME shipped script is what keeps the two from diverging, so hold
/// the shell byte-identical: an edit to one path that skips the other fails here.
#[cfg(unix)]
#[test]
fn pages_build_gate_is_identical_on_both_deploy_paths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let direct = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let coalesced = std::fs::read_to_string(root.join("visual-docs-pages/action.yml")).unwrap();

    for step in [
        "Record the current Pages build",
        "Check whether anything was published",
        "Wait for the Pages build",
    ] {
        assert_eq!(
            action_step_script(&direct, step),
            action_step_script(&coalesced, step),
            "the '{step}' step must be the same shell on both deploy paths"
        );
    }

    // Order is the contract: read the build id and branch head BEFORE the push,
    // compare and wait after it. A gate that ran before its own deploy would
    // observe the previous run's build and pass on a broken one.
    let index = |needle: &str| direct.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(
        index("    - name: Record the current Pages build")
            < index("    - name: Deploy canonical gallery")
            && index("    - name: Deploy PR preview gallery externally")
                < index("    - name: Check whether anything was published")
            && index("    - name: Check whether anything was published")
                < index("    - name: Wait for the Pages build"),
        "the direct deploy must be sandwiched by its build gate"
    );
    // The build settling is what the preview URL is waiting on, so gate first.
    assert!(
        index("    - name: Wait for the Pages build")
            < index("    - name: Wait for the PR preview to go live")
    );
    // The gate reads the branch peaceiris writes, and the direct deploys leave
    // `publish_branch` at its default — so no new input, and no way to configure
    // the two out of step.
    assert!(
        !direct.contains("publish_branch:") && direct.contains("BRANCH: gh-pages"),
        "the direct deploy's gallery branch is peaceiris's default"
    );
}

/// Drive the direct per-lane deploy's gate end to end: the shipped step scripts,
/// a real local git remote for the branch-head reads, and a stub `gh` for the
/// build status. An errored build must fail the lane rather than let it return
/// green with the gallery unpublished.
#[cfg(unix)]
#[test]
fn direct_per_lane_deploy_fails_when_its_pages_build_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let dir = TempDir::new().unwrap();
    let remote = dir.path().join("gallery");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::write(remote.join("index.html"), "gallery").unwrap();

    let git = |args: Vec<&str>| {
        assert!(
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&remote)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    };
    git(vec!["init", "-q"]);
    git(vec!["config", "user.name", "Test"]);
    git(vec!["config", "user.email", "test@example.com"]);
    git(vec!["add", "."]);
    git(vec!["commit", "-qm", "gallery"]);
    git(vec!["branch", "-M", "gh-pages"]);

    // Only the remote URL is substituted; every decision below is the shipped
    // shell. `$GITHUB_ACTION_PATH` is where the runner unpacks the action, which
    // is how it reaches the sibling script.
    let local = |step: &str| {
        action_step_script(&action, step).replace(
            "\"https://x-access-token:${GH_TOKEN}@github.com/${REPO}.git\"",
            &format!("\"{}\"", remote.display()),
        )
    };
    let step = |script: &str, label: &str, polls: &[&str], envs: &[(&str, &str)]| {
        let work = dir.path().join(label);
        let stub = write_gh_stub(&work, polls);
        let output_file = dir.path().join(format!("out-{label}"));
        std::fs::write(&output_file, "").unwrap();
        let mut command = std::process::Command::new("bash");
        command
            .arg("-c")
            .arg(script)
            .env("WORK", &work)
            .env("GH_BIN", &stub)
            .env("GITHUB_ACTION_PATH", root.join("visual-docs"))
            .env("REPO", "docs/galleries")
            .env("BRANCH", "gh-pages")
            .env("GH_TOKEN", "token")
            .env("POLL_SECONDS", "0")
            .env("APPEAR_ATTEMPTS", "3")
            .env("SETTLE_ATTEMPTS", "3")
            .env("GITHUB_OUTPUT", &output_file);
        for (key, value) in envs {
            command.env(key, value);
        }
        let result = command.output().unwrap();
        (
            result,
            std::fs::read_to_string(&output_file).unwrap(),
            gh_stub_rebuilds(&work),
        )
    };

    // Before the push: the build already published, plus the branch head.
    let (recorded, outputs, _) = step(
        &local("Record the current Pages build"),
        "before",
        &["100 built"],
        &[],
    );
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    assert!(outputs.contains("build=100"), "{outputs}");
    let before = output_value(&outputs, "head");
    assert!(!before.is_empty(), "{outputs}");

    // peaceiris pushes this lane's gallery.
    std::fs::write(remote.join("index.html"), "new gallery").unwrap();
    git(vec!["add", "."]);
    git(vec!["commit", "-qm", "deploy"]);

    let (checked, outputs, _) = step(
        &local("Check whether anything was published"),
        "published",
        &["100 built"],
        &[("BEFORE", &before)],
    );
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(outputs.contains("published=true"), "{outputs}");

    // The gate. A build that errors even after the one retry means this lane's
    // gallery was never published, so the lane must go red.
    let (failed, _, rebuilds) = step(
        &local("Wait for the Pages build"),
        "errored",
        &["101 errored", "101 errored", "102 errored"],
        &[("PREVIOUS_BUILD", "100")],
    );
    assert!(
        !failed.status.success(),
        "a direct per-lane deploy must fail when its Pages build errors"
    );
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        stderr.contains("::error::") && stderr.contains("the published gallery is stale"),
        "{stderr}"
    );
    assert_eq!(rebuilds, 1, "a superseded build is retried exactly once");

    // The same wiring passes once the build settles, so the gate costs a healthy
    // lane nothing but the wait.
    let (ok, _, rebuilds) = step(
        &local("Wait for the Pages build"),
        "built",
        &["101 building", "101 built"],
        &[("PREVIOUS_BUILD", "100")],
    );
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("pages build succeeded"));
    assert_eq!(rebuilds, 0);

    // A re-run that publishes nothing pushes no commit, so there is no build to
    // wait for and the gate is skipped rather than blaming the Pages source.
    let (noop, outputs, _) = step(
        &local("Check whether anything was published"),
        "noop",
        &["100 built"],
        &[("BEFORE", &head_of(&remote))],
    );
    assert!(noop.status.success());
    assert!(outputs.contains("published=false"), "{outputs}");
}

/// The current commit of a local git repository.
#[cfg(unix)]
fn head_of(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

// A capture container bind-mounts the consumer's working tree, so it runs as the
// invoking user rather than root. That mapping works only as a package of four:
//
//   1. `--user <uid>:<gid>` from the host;
//   2. the `/work/node_modules` mask is a host directory the caller created (an
//      anonymous Docker volume is created root-owned, so `npm ci` gets EACCES);
//   3. `HOME` points at a host directory the caller created (the mapped uid has
//      no passwd entry in the image, so npm resolves no writable home);
//   4. whatever created that scratch removes it however it exits.
//
// Five files publish that invocation and nothing reconciles them, so the checks
// below hold each to the contract rather than to another copy's text. The
// container boundary itself is proven by the demo journey AGENTS.md requires
// before release; this suite runs none.

/// Shell logical lines: comments dropped (they quote the *old* form as the
/// anti-pattern) and `\`-continuations joined, so one `docker run` is one line.
fn shell_logical_lines(script: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for raw in script.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        match line.strip_suffix('\\') {
            Some(head) => {
                pending.push_str(head.trim_end());
                pending.push(' ');
            }
            None => {
                pending.push_str(line);
                lines.push(std::mem::take(&mut pending));
            }
        }
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

/// Split a `docker run` line into flags and values, keeping a `$(...)`
/// substitution whole: `--user "$(id -u):$(id -g)"` is one value, not three.
fn docker_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut depth = 0usize;
    let mut previous = ' ';
    for c in line.chars() {
        match c {
            '(' if previous == '$' => {
                depth += 1;
                token.push(c);
            }
            ')' if depth > 0 => {
                depth -= 1;
                token.push(c);
            }
            _ if c.is_whitespace() && depth == 0 => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(c),
        }
        previous = c;
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn unquote(token: &str) -> String {
    token.trim_matches(|c| c == '"' || c == '\'').to_owned()
}

/// The values a repeated `docker run` flag was given (`-v`, `--user`, `-e`).
fn flag_values(tokens: &[String], flag: &str) -> Vec<String> {
    tokens
        .windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| unquote(&pair[1]))
        .collect()
}

/// The right-hand side of `name=...`, so the check works whatever the copy calls
/// its variables.
fn shell_assignment(lines: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    lines
        .iter()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(unquote))
}

/// The first `$var` / `${var}` referenced in a word, e.g. the scratch variable in
/// `"$capture_scratch/node_modules:/work/node_modules"`.
fn first_shell_var(word: &str) -> Option<String> {
    let after = &word[word.find('$')? + 1..];
    let name: String = after
        .strip_prefix('{')
        .unwrap_or(after)
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `${var}` and `$var` are the same reference; compare paths in one spelling.
fn normalize_vars(word: &str) -> String {
    word.replace("${", "$").replace('}', "")
}

fn references(word: &str, var: &str) -> bool {
    normalize_vars(word).contains(&format!("${var}"))
}

/// Every directory the script creates on the host, so the check can ask whether
/// each mount the container needs already exists (Docker creates a missing one
/// as root).
fn host_dirs_created(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.trim_start().starts_with("mkdir"))
        .flat_map(|line| line.split_whitespace().skip(1))
        .filter(|arg| !arg.starts_with('-'))
        .map(unquote)
        .collect()
}

/// Who removes the host scratch a copy creates: the script itself, or the caller
/// it hands the tree back to — a CI runner that discards its whole workspace, or
/// a reader following a commented example in their own shell.
#[derive(Clone, Copy)]
enum Scratch {
    RemovedByTheScript,
    ReclaimedByTheCaller,
}

/// Hold one capture script to the four-part contract above. Semantic, not
/// textual: variable names, ordering, wording and mount points are all the
/// copy's own choice — losing any of the four parts is what fails.
fn assert_capture_runs_as_the_host_user(label: &str, script: &str, scratch_owner: Scratch) {
    let lines = shell_logical_lines(script);
    let docker = docker_tokens(
        lines
            .iter()
            .find(|line| line.contains("docker run"))
            .unwrap_or_else(|| panic!("{label}: no `docker run` capture invocation")),
    );

    // 1. The mapping may be written inline or reached through a variable the
    //    script assigns, so follow one level of assignment before concluding it
    //    is not the host's ids.
    let users = flag_values(&docker, "--user");
    let user = match users.as_slice() {
        [only] => only.clone(),
        _ => panic!("{label}: capture container does not run as the host user (no --user)"),
    };
    let maps_host_ids = |value: &str| value.contains("id -u") && value.contains("id -g");
    let user_is_host = maps_host_ids(&user)
        || first_shell_var(&user)
            .and_then(|var| shell_assignment(&lines, &var))
            .is_some_and(|value| maps_host_ids(&value));
    assert!(
        user_is_host,
        "{label}: --user {user} is not the invoking host uid:gid"
    );

    // 2. node_modules is masked by a host directory the script created — never an
    //    anonymous volume, which under --user is root-owned and unwritable.
    let mounts = flag_values(&docker, "-v");
    assert!(
        !mounts.iter().any(|mount| !mount.contains(':')),
        "{label}: anonymous volume mount cannot be written under --user: {mounts:?}"
    );
    let mask = mounts
        .iter()
        .find(|mount| mount.ends_with(":/work/node_modules"))
        .unwrap_or_else(|| panic!("{label}: nothing masks /work/node_modules: {mounts:?}"));
    let mask_source = mask.rsplit_once(':').expect("mount has a destination").0;
    let scratch = first_shell_var(mask_source).unwrap_or_else(|| {
        panic!("{label}: the node_modules mask {mask_source} is not a host scratch directory")
    });
    let scratch_value = shell_assignment(&lines, &scratch)
        .unwrap_or_else(|| panic!("{label}: ${scratch} is never assigned"));
    assert!(
        scratch_value.contains("mktemp -d"),
        "{label}: ${scratch} is not a private host scratch directory: {scratch_value}"
    );
    let created = host_dirs_created(&lines);
    assert!(
        created
            .iter()
            .any(|dir| references(dir, &scratch) && dir.ends_with("/node_modules")),
        "{label}: the node_modules mask is never created on the host, so Docker \
         creates it root-owned: {created:?}"
    );
    // The mask's destination is inside the bind-mounted tree, and Docker
    // materializes a missing bind-mount destination as root — leaving exactly the
    // residue the user mapping exists to prevent. The caller creates it too.
    assert!(
        created.iter().any(|dir| !references(dir, &scratch)
            && (dir == "node_modules" || dir.ends_with("/node_modules"))),
        "{label}: the /work/node_modules mountpoint is never created in the tree, \
         so Docker creates it root-owned under the bind mount: {created:?}"
    );

    // 3. HOME is a host directory under that scratch, so the package manager has
    //    somewhere to write (the mapped uid has no passwd entry in the image).
    let home = flag_values(&docker, "-e")
        .into_iter()
        .find_map(|env| env.strip_prefix("HOME=").map(str::to_owned))
        .unwrap_or_else(|| panic!("{label}: capture container sets no writable HOME"));
    let (home_source, home_dest) = mounts
        .iter()
        .filter_map(|mount| mount.rsplit_once(':'))
        .find(|(_, dest)| home == *dest || home.starts_with(&format!("{dest}/")))
        .unwrap_or_else(|| panic!("{label}: HOME={home} is not on a host mount: {mounts:?}"));
    assert_eq!(
        first_shell_var(home_source).as_deref(),
        Some(scratch.as_str()),
        "{label}: HOME={home} is not backed by the ${scratch} host scratch"
    );
    let home_on_host = normalize_vars(&format!("{home_source}{}", &home[home_dest.len()..]));
    assert!(
        created
            .iter()
            .any(|dir| normalize_vars(dir) == home_on_host),
        "{label}: the HOME directory {home_on_host} is never created on the host, \
         so Docker creates it root-owned: {created:?}"
    );

    // 4. Whatever created the scratch removes it, however the script exits —
    //    except where the caller reclaims the whole tree around it instead.
    if matches!(scratch_owner, Scratch::ReclaimedByTheCaller) {
        return;
    }
    assert!(
        lines.iter().any(|line| {
            line.contains("trap")
                && line.contains("rm -rf")
                && line.contains(&scratch)
                && line.trim_end().ends_with("EXIT")
        }),
        "{label}: ${scratch} is not removed on exit"
    );
}

#[test]
fn scaffolded_hook_captures_as_the_host_user() {
    let dir = TempDir::new().unwrap();
    let root = path_str(dir.path());
    invoke(&["screencomp", "init", "--dir", &root]).0.unwrap();
    let hook = std::fs::read_to_string(dir.path().join(".githooks/pre-push")).unwrap();
    assert_capture_runs_as_the_host_user(
        "the hook `screencomp init` scaffolds",
        &hook,
        Scratch::RemovedByTheScript,
    );
}

#[test]
fn example_hook_captures_as_the_host_user() {
    // Not documentation: sync-demo.yml installs this file verbatim as the demo
    // repository's real .githooks/pre-push, so a regression here is a live defect.
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/pre-push");
    let hook = std::fs::read_to_string(&example)
        .expect("the copy-paste hook template must exist in this repo");
    assert_capture_runs_as_the_host_user("examples/pre-push", &hook, Scratch::RemovedByTheScript);
}

/// A file this repository ships, read from the checkout rather than a fixture:
/// the copies below are held to the contract as they are published.
fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{relative}: {err}"))
}

/// The shipped text from `start` through `end`, lifted out of the column its
/// file keeps it in — YAML block indentation, the `#` of a commented example, or
/// a fenced snippet's margin — so what is left is the shell the copy publishes.
fn shipped_block(source: &str, start: &str, end: &str) -> String {
    let start_at = source
        .find(start)
        .unwrap_or_else(|| panic!("no shipped block starting `{start}`"));
    let line_at = source[..start_at]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let margin = &source[line_at..start_at];
    let block = &source[line_at..];
    let end_at = block
        .find(end)
        .unwrap_or_else(|| panic!("no shipped block ending `{end}`"))
        + end.len();
    block[..end_at]
        .lines()
        .map(|line| {
            line.strip_prefix(margin)
                .unwrap_or_else(|| line.trim_start())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One shipped copy of the capture invocation: the shell it publishes, whatever
/// its file sets around that shell, and who owns the scratch it creates.
struct CaptureCopy {
    label: &'static str,
    script: String,
    scratch_owner: Scratch,
}

/// The reseed capture in `sync-demo.yml`, composed from the two places the
/// workflow keeps it: the setup runs once, the `docker run` once per arch inside
/// the loop between them.
fn sync_demo_capture() -> CaptureCopy {
    let workflow = repo_file(".github/workflows/sync-demo.yml");
    let setup = shipped_block(
        &workflow,
        r#"host_user="$(id -u):$(id -g)""#,
        r#""$scratch/home" node_modules"#,
    );
    let capture = shipped_block(
        &workflow,
        r#"docker run --rm --platform="$platform""#,
        r#"bash capture.sh""#,
    );
    CaptureCopy {
        label: ".github/workflows/sync-demo.yml",
        script: format!("{setup}\n{capture}"),
        // A reseed runs in a throwaway job workspace the runner discards whole.
        scratch_owner: Scratch::ReclaimedByTheCaller,
    }
}

#[test]
fn every_shipped_capture_copy_runs_as_the_host_user() {
    // The two executable hooks have their own tests above; these three are the
    // copies a reader or a runner follows instead, and they drift from the hooks
    // the same way — silently, one file at a time.
    let readme = repo_file("README.md");
    let example_workflow = repo_file("examples/visual-docs.yml");
    let copies = [
        sync_demo_capture(),
        CaptureCopy {
            label: "README.md",
            script: shipped_block(
                &readme,
                r#"scratch="$(mktemp -d)"; trap"#,
                "npx playwright test'",
            ),
            scratch_owner: Scratch::RemovedByTheScript,
        },
        CaptureCopy {
            label: "examples/visual-docs.yml",
            script: shipped_block(
                &example_workflow,
                r#"scratch="$(mktemp -d)""#,
                r#"rm -rf "$scratch""#,
            ),
            // A reader pastes this into their own shell and removes the scratch
            // with the `rm -rf` the example ends on.
            scratch_owner: Scratch::ReclaimedByTheCaller,
        },
    ];
    for copy in &copies {
        assert_capture_runs_as_the_host_user(copy.label, &copy.script, copy.scratch_owner);
    }
}

#[test]
fn the_in_container_capture_script_needs_no_root() {
    // demo/capture.sh runs *inside* the capture container, which now runs under
    // the caller's uid: an install step that shells out to the system package
    // manager (`playwright install --with-deps`) would fail there, and the pinned
    // image already ships the browser and its dependencies.
    let script = shell_logical_lines(&repo_file("demo/capture.sh")).join("\n");
    for root_only in ["--with-deps", "sudo ", "apt-get", "apt "] {
        assert!(
            !script.contains(root_only),
            "demo/capture.sh runs as the invoking uid inside the container, which cannot `{root_only}`: {script}"
        );
    }
}
