//! End-to-end tests that execute the compiled `screencomp` binary.
//!
//! These cover critical user journeys from the user's perspective — exit codes,
//! stdout/stderr separation, and file effects — not just "the binary starts".
// llmlint: ignore-file[tests_mirror_real_usage] The visual-docs acceptance test intentionally extracts and composes the shipped action's fetch/build blocks: GitHub exposes no offline composite-action runner, and executing these exact blocks together is the requested CI-path boundary without remote side effects.

use std::path::{Path, PathBuf};

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

/// Write a single-shot capture under `dir` (the common scoping-test shape).
fn write_one(dir: &Path, name: &str, hash: &str, bytes: &[u8]) {
    write_capture(
        dir,
        &[(name, &[("viewport", "desktop")], hash, "home.png", bytes)],
    );
}

#[test]
fn demo_managed_config_is_valid_under_current_schema() {
    // `demo/screencomp.toml` is the source of truth synced to screencomp-demo by
    // sync-demo.yml. A config-schema change that breaks it must fail HERE, in this
    // repo's gate, rather than silently ship a broken consumer. `success` proves it
    // parses under the current schema; the arch proves the CI matrix is populated.
    let cfg = format!("{}/demo/screencomp.toml", env!("CARGO_MANIFEST_DIR"));
    bin()
        .args(["--config", &cfg, "arches", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("x86_64"));
}

#[test]
fn help_lists_subcommands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: screencomp"))
        .stdout(predicate::str::contains("index"))
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("gallery"))
        .stdout(predicate::str::contains("comment"))
        .stdout(predicate::str::contains("verify"))
        .stdout(predicate::str::contains("doctor"));
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
        .stdout(predicate::str::contains("changed about [viewport=desktop]"))
        .stdout(predicate::str::contains("added pricing [viewport=desktop]"))
        .stdout(predicate::str::contains(
            "added 1 changed 1 removed 0 unchanged 2",
        ))
        // A `changed` shot earns the cross-CPU-drift hint on stdout (human
        // output), never stderr — the stream split must hold.
        .stdout(predicate::str::contains("cross-CPU anti-aliasing drift"))
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
        .stdout(predicate::str::contains(r#""status":"added""#))
        // The toggle map is part of each entry now, not a `project` field.
        .stdout(predicate::str::contains(r#""toggles":{"viewport":"desktop"}"#))
        // The JSON contract stays a clean single machine line: the human-only
        // cross-CPU hint must never leak into it.
        .stdout(predicate::str::contains("cross-CPU").not());
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
    // Without a gallery URL the comment is a label listing, not inline images.
    assert!(!md.contains("<img"));
}

#[test]
fn comment_embeds_inline_previews_when_gallery_url_given() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("comment.md");

    bin()
        .args(["comment", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .arg("--gallery-url")
        .arg("https://example.test/pr/12/")
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let md = std::fs::read_to_string(&out).expect("comment file");
    // Small diff under the default limit: inline before/after images appear.
    assert!(md.contains("| Before | After |"));
    assert!(md.contains("src=\"https://example.test/pr/12/baseline/about-desktop.png\""));
    assert!(md.contains("src=\"https://example.test/pr/12/current/pricing-desktop.png\""));
    assert!(md.contains("width=\"380\""));
}

#[test]
fn comment_manifest_mode_embeds_current_only_from_gallery_url() {
    // The headline image-free feature: with a digest-manifest baseline there are
    // no baseline PNGs to host, so a `--gallery-url` (a plain gallery of the
    // current shots) must embed only "After" images at `<URL>/<image>` — never a
    // `baseline/` URL that would 404 in the rendered comment.
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("baseline.json");
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success();

    let out = dir.path().join("comment.md");
    bin()
        .args(["comment", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .arg("--gallery-url")
        .arg("https://example.test/site/")
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let md = std::fs::read_to_string(&out).expect("comment file");
    assert!(
        md.contains("src=\"https://example.test/site/about-desktop.png\""),
        "{md}"
    );
    assert!(!md.contains("/baseline/"), "{md}");
    assert!(!md.contains("/current/"), "{md}");
}

#[test]
fn comment_aggregated_upserts_one_comment_across_projects() {
    // A many-project monorepo: two affected projects folded into ONE comment,
    // keyed by a single stable marker, driven through the compiled binary.
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let vp: &[(&str, &str)] = &[("viewport", "desktop")];

    // app-web: an image-tree baseline vs a current with a changed + an added shot.
    write_capture(
        &root.join("app-web/baseline"),
        &[("home", vp, &digest("33"), "home.png", b"old")],
    );
    write_capture(
        &root.join("app-web/current"),
        &[
            ("home", vp, &digest("11"), "home.png", b"new"),
            ("pricing", vp, &digest("22"), "pricing.png", b"add"),
        ],
    );
    // app-admin: unchanged capture (affected by a path change, but no visual diff).
    write_capture(
        &root.join("app-admin/baseline"),
        &[("home", vp, &digest("aa"), "home.png", b"h")],
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
                {{"id":"app-web","baseline":{web_b:?},"current":{web_c:?},
                  "gallery_url":"https://example.test/pr-1/app-web",
                  "baseline_url":"https://example.test/pr-1/app-web/baseline",
                  "current_url":"https://example.test/pr-1/app-web/current"}},
                {{"id":"app-admin","baseline":{adm_b:?},"current":{adm_c:?},
                  "gallery_url":"https://example.test/pr-1/app-admin",
                  "baseline_url":"https://example.test/pr-1/app-admin/baseline",
                  "current_url":"https://example.test/pr-1/app-admin/current"}}
              ]
            }}"#,
            web_b = root.join("app-web/baseline").to_str().unwrap(),
            web_c = root.join("app-web/current").to_str().unwrap(),
            adm_b = root.join("app-admin/baseline").to_str().unwrap(),
            adm_c = root.join("app-admin/current").to_str().unwrap(),
        ),
    )
    .unwrap();

    let out = root.join("comment.md");
    bin()
        .args(["comment", "--projects"])
        .arg(&spec)
        .arg("--output")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));

    let md = std::fs::read_to_string(&out).expect("comment file");
    assert!(md.starts_with("<!-- screencomp-aggregate -->"), "{md}");
    assert_eq!(md.matches("<!--").count(), 1, "exactly one marker: {md}");
    assert!(
        md.contains(
            "**1 project with visual changes · 1 project unchanged · 1 added · 1 changed · 0 removed**"
        ),
        "{md}"
    );
    assert!(!md.contains("| Project |"), "{md}");
    assert!(!md.contains("### app-admin"), "{md}");
    assert!(
        md.contains("src=\"https://example.test/pr-1/app-web/baseline/home.png\""),
        "{md}"
    );
    assert!(
        md.contains("src=\"https://example.test/pr-1/app-web/current/home.png\""),
        "{md}"
    );
}

#[test]
fn comment_aggregated_links_focused_diffs_over_the_limit() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let vp: &[(&str, &str)] = &[("viewport", "desktop")];
    write_capture(
        &root.join("baseline"),
        &[("home", vp, &digest("aa"), "home.png", b"old")],
    );
    write_capture(
        &root.join("current"),
        &[
            ("home", vp, &digest("bb"), "home.png", b"new"),
            ("added", vp, &digest("cc"), "added.png", b"add"),
        ],
    );
    let spec = root.join("projects.json");
    std::fs::write(
        &spec,
        format!(
            r#"{{"schema":2,"projects":[{{"id":"app","baseline":{baseline:?},
            "current":{current:?},
            "baseline_url":"https://example.test/pr-1/app/baseline",
            "current_url":"https://example.test/pr-1/app/current"}}]}}"#,
            baseline = root.join("baseline").to_str().unwrap(),
            current = root.join("current").to_str().unwrap(),
        ),
    )
    .unwrap();
    let config = root.join("screencomp.toml");
    std::fs::write(&config, "[comment]\nembed_limit = 1\n").unwrap();

    bin()
        .args(["--config"])
        .arg(&config)
        .args(["comment", "--projects"])
        .arg(&spec)
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "over-limit aggregated comments require a focused-diff gallery URL",
        ));

    std::fs::write(
        &spec,
        format!(
            r#"{{"schema":2,"projects":[{{"id":"app","baseline":{baseline:?},
            "current":{current:?},"gallery_url":"https://example.test/pr-1/app",
            "baseline_url":"https://example.test/pr-1/app/baseline",
            "current_url":"https://example.test/pr-1/app/current"}}]}}"#,
            baseline = root.join("baseline").to_str().unwrap(),
            current = root.join("current").to_str().unwrap(),
        ),
    )
    .unwrap();

    let output = bin()
        .args(["--config"])
        .arg(&config)
        .args(["comment", "--projects"])
        .arg(&spec)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let md = String::from_utf8(output).unwrap();
    assert!(md.contains("| Project |"), "{md}");
    assert!(
        md.contains("[View focused diff](https://example.test/pr-1/app)"),
        "{md}"
    );
    assert!(!md.contains("<img"), "{md}");
}

#[test]
fn comment_projects_conflicts_with_single_project_inputs() {
    // `--projects` and the single-project inputs are mutually exclusive; clap
    // rejects the combination as a usage error (exit 2) before any work.
    bin()
        .args(["comment", "--projects", "spec.json", "--baseline"])
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn comment_projects_rejects_invalid_baseline_choice_then_accepts_one_source() {
    let dir = TempDir::new().unwrap();
    let spec = dir.path().join("projects.json");
    let baseline = baseline().to_str().unwrap().to_owned();
    let current = current().to_str().unwrap().to_owned();

    for (project_fields, expected) in [
        (
            format!(r#""baseline":{baseline:?},"baseline_manifest":"baseline.json","#),
            "sets both `baseline` and `baseline_manifest`; use exactly one",
        ),
        ("".to_owned(), "needs a `baseline` or `baseline_manifest`"),
    ] {
        std::fs::write(
            &spec,
            format!(
                r#"{{"schema":2,"projects":[{{"id":"app",{project_fields}"current":{current:?}}}]}}"#
            ),
        )
        .unwrap();
        bin()
            .args(["comment", "--projects"])
            .arg(&spec)
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(expected));
    }

    std::fs::write(
        &spec,
        format!(
            r#"{{"schema":2,"projects":[{{"id":"app","baseline":{baseline:?},"current":{current:?}}}]}}"#
        ),
    )
    .unwrap();
    bin()
        .args(["comment", "--projects"])
        .arg(&spec)
        .assert()
        .success()
        .stdout(predicate::str::contains("## Visual changes"))
        .stderr(predicate::str::is_empty());
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
    assert!(html.contains("<h2>about</h2>"));
    assert!(html.contains("src=\"about-desktop.png\""));

    // The gallery is self-contained: every referenced image is copied alongside
    // index.html with identical bytes, so the directory is deploy-ready.
    let copied = std::fs::read(dir.path().join("about-desktop.png")).expect("image copied");
    let source = std::fs::read(current().join("about-desktop.png")).expect("source image");
    assert_eq!(copied, source);
    assert_eq!(
        std::fs::read(dir.path().join("captures.json")).unwrap(),
        std::fs::read(current().join("captures.json")).unwrap()
    );
}

#[test]
fn gallery_renders_toggle_controls_from_declared_dimensions() {
    // With a `viewport` dimension declared, the multi-value `home` name gets a
    // toggle control with the expected data attributes; the page deploys as-is.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nlabel = \"Viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();
    let out = dir.path().join("site");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let html = std::fs::read_to_string(out.join("index.html")).expect("index.html");
    assert!(html.contains("data-dim=\"viewport\""), "{html}");
    assert!(html.contains("data-val=\"mobile\""), "{html}");
    assert!(html.contains("data-variant=\"viewport=mobile\""), "{html}");
}

#[test]
fn gallery_has_one_toggle_bar_that_filters_cards() {
    // The page carries a single page-wide toggle bar (not one per card), and the
    // default selection filters the cards: a name with only the non-default value
    // is hidden server-side so the gallery is usable without JavaScript.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nlabel = \"Viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();
    let input = dir.path().join("shots");
    write_capture(
        &input,
        &[
            (
                "home",
                &[("viewport", "desktop")],
                &"a".repeat(64),
                "home-d.png",
                b"d",
            ),
            (
                "home",
                &[("viewport", "mobile")],
                &"b".repeat(64),
                "home-m.png",
                b"m",
            ),
            (
                "legacy",
                &[("viewport", "mobile")],
                &"c".repeat(64),
                "legacy-m.png",
                b"l",
            ),
        ],
    );
    let out = dir.path().join("site");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["gallery", "--input"])
        .arg(&input)
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let html = std::fs::read_to_string(out.join("index.html")).expect("index.html");
    // Exactly one toggle bar for the whole page, not one repeated per card.
    assert_eq!(html.matches("class=\"toggles\"").count(), 1, "{html}");
    // Default selection is `desktop`; `legacy` only has `mobile`, so its card is
    // filtered out (hidden) while `home` (which has a desktop variant) stays.
    assert!(
        html.contains("<section class=\"shot\" hidden><h2>legacy</h2>"),
        "{html}"
    );
    assert!(
        html.contains("<section class=\"shot\"><h2>home</h2>"),
        "{html}"
    );
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
    assert!(html.contains("src=\"baseline/about-desktop.png\""));
    assert!(html.contains("src=\"current/about-desktop.png\""));
    // Both image trees are copied so before/after both render.
    assert!(dir.path().join("baseline/about-desktop.png").exists());
    assert!(dir.path().join("current/about-desktop.png").exists());
}

#[test]
fn deployed_canonical_gallery_drives_a_focused_preview_diff() {
    let dir = TempDir::new().unwrap();
    let canonical = dir.path().join("canonical");
    let preview = dir.path().join("preview");

    // This is the same two-command boundary the Pages workflow uses: first
    // publish a plain canonical gallery, then consume its deployed subtree as
    // the next run's baseline.
    bin()
        .args(["gallery", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&canonical)
        .assert()
        .success();
    bin()
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--baseline")
        .arg(&canonical)
        .arg("--focused")
        .arg("--output")
        .arg(&preview)
        .assert()
        .success();

    let html = std::fs::read_to_string(preview.join("index.html")).expect("preview");
    assert!(html.contains("<h2>Changed</h2>"), "{html}");
    assert!(html.contains("<h2>Added</h2>"), "{html}");
    assert!(html.contains("<summary>Unchanged ("), "{html}");
    assert!(!html.contains("<h2>Unchanged</h2>"), "{html}");
    assert!(preview.join("baseline/captures.json").exists());
    assert!(preview.join("current/captures.json").exists());
    assert!(preview.join("baseline/about-desktop.png").exists());
    assert!(preview.join("current/about-desktop.png").exists());
}

#[test]
fn focused_gallery_requires_a_baseline() {
    bin()
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--focused")
        .arg("--output")
        .arg(TempDir::new().unwrap().path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "required arguments were not provided",
        ))
        .stderr(predicate::str::contains("--baseline <DIR>"));
}

#[cfg(unix)]
#[test]
fn shipped_pr_preview_shell_builds_focused_diff_and_recovers_without_canonical() {
    let dir = TempDir::new().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let action = std::fs::read_to_string(root.join("visual-docs/action.yml")).unwrap();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_screencomp"));
    let binary_dir = binary.parent().unwrap();
    let path = std::env::join_paths(std::iter::once(binary_dir.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();

    let canonical = dir.path().join("canonical-repository");
    std::fs::create_dir_all(&canonical).unwrap();
    assert!(
        std::process::Command::new(&binary)
            .args(["gallery", "--input"])
            .arg(baseline())
            .arg("--output")
            .arg(&canonical)
            .status()
            .unwrap()
            .success()
    );
    for args in [
        ["init", "-q"].as_slice(),
        ["config", "user.name", "Test"].as_slice(),
        ["config", "user.email", "test@example.com"].as_slice(),
        ["add", "."].as_slice(),
        ["commit", "-qm", "canonical"].as_slice(),
        ["branch", "-M", "gh-pages"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&canonical)
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
    let fetch_end = action[fetch_run..].find("\n    - name:").unwrap() + fetch_run;
    let fetch_script = action[fetch_run..fetch_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
        .replace(
            "\"https://github.com/${PAGES_REPO}.git\"",
            &format!("\"{}\"", canonical.display()),
        );
    let build_step = action.find("    - name: Build gallery").unwrap();
    let build_run =
        action[build_step..].find("      run: |\n").unwrap() + build_step + "      run: |\n".len();
    let build_end = action[build_run..].find("\n    - name:").unwrap() + build_run;
    let build_script = action[build_run..build_end]
        .lines()
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");

    let preview_work = dir.path().join("preview-work");
    std::fs::create_dir_all(&preview_work).unwrap();
    let fetch_output = preview_work.join("fetch-output");
    let fetched = std::process::Command::new("bash")
        .arg("-c")
        .arg(&fetch_script)
        .current_dir(&preview_work)
        .env("PAGES_REPO", "docs/galleries")
        .env("PAGES_TOKEN", "token")
        .env("DEST", "")
        .env("ARCH", "")
        .env("RUNNER_TEMP", dir.path())
        .env("GITHUB_OUTPUT", &fetch_output)
        .output()
        .unwrap();
    assert!(
        fetched.status.success(),
        "{}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    let fetch_outputs = std::fs::read_to_string(&fetch_output).unwrap();
    let baseline_path = fetch_outputs
        .lines()
        .find_map(|line| line.strip_prefix("path="))
        .unwrap();
    let built = std::process::Command::new("bash")
        .arg("-c")
        .arg(&build_script)
        .current_dir(&preview_work)
        .env("PATH", &path)
        .env("CURRENT", current())
        .env("ARCH", "")
        .env("GALLERY_TITLE", "PR preview")
        .env("BASELINE_FOUND", "true")
        .env("BASELINE_PATH", baseline_path)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let site = preview_work.join("site");
    let html = std::fs::read_to_string(site.join("index.html")).unwrap();
    assert!(html.contains("<h2>Changed</h2>"), "{html}");
    assert!(html.contains("<summary>Unchanged ("), "{html}");
    assert!(!html.contains("<h2>Unchanged</h2>"), "{html}");
    for file in [
        "baseline/captures.json",
        "current/captures.json",
        "baseline/about-desktop.png",
        "current/about-desktop.png",
    ] {
        assert!(site.join(file).is_file(), "{file}");
    }

    let no_canonical = dir.path().join("repository-without-gh-pages");
    std::fs::create_dir_all(&no_canonical).unwrap();
    std::fs::write(no_canonical.join("README"), "seed").unwrap();
    for args in [
        ["init", "-q"].as_slice(),
        ["config", "user.name", "Test"].as_slice(),
        ["config", "user.email", "test@example.com"].as_slice(),
        ["add", "."].as_slice(),
        ["commit", "-qm", "seed"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&no_canonical)
                .status()
                .unwrap()
                .success()
        );
    }
    let recovery_fetch = fetch_script.replace(
        &canonical.display().to_string(),
        &no_canonical.display().to_string(),
    );
    let recovery_work = dir.path().join("recovery-work");
    std::fs::create_dir_all(&recovery_work).unwrap();
    let recovery_output = recovery_work.join("fetch-output");
    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(recovery_fetch)
            .current_dir(&recovery_work)
            .env("PAGES_REPO", "docs/galleries")
            .env("PAGES_TOKEN", "token")
            .env("DEST", "")
            .env("ARCH", "")
            .env("RUNNER_TEMP", dir.path())
            .env("GITHUB_OUTPUT", &recovery_output)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(&recovery_output).unwrap(),
        "found=false\n"
    );
    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(&build_script)
            .current_dir(&recovery_work)
            .env("PATH", &path)
            .env("CURRENT", current())
            .env("ARCH", "")
            .env("GALLERY_TITLE", "First preview")
            .env("BASELINE_FOUND", "false")
            .env("BASELINE_PATH", "")
            .status()
            .unwrap()
            .success()
    );
    let recovery_site = recovery_work.join("site");
    let recovery_html = std::fs::read_to_string(recovery_site.join("index.html")).unwrap();
    assert!(!recovery_html.contains("<h2>Changed</h2>"));
    assert!(recovery_site.join("captures.json").is_file());
    assert!(recovery_site.join("about-desktop.png").is_file());
    assert!(!recovery_site.join("baseline").exists());
}

/// Host CPU arch, mirroring `commands::arch::host_arch`.
fn host_arch() -> String {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
    .to_owned()
}

#[test]
fn classify_arch_auto_compares_only_the_host_subtree() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let key = host_arch();

    // Host subtree changes; a foreign subtree differs too but must be ignored.
    write_one(&base.join(&key), "home", &digest("aa"), b"old");
    write_one(&cur.join(&key), "home", &digest("bb"), b"new");
    write_one(&base.join("other-arch"), "home", &digest("11"), b"a");
    write_one(&cur.join("other-arch"), "home", &digest("22"), b"b");

    bin()
        .args(["classify", "--arch", "auto", "--exit-code", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .code(3)
        .stdout(predicate::str::contains("changed home [viewport=desktop]"))
        // Only the host subtree is compared (1 shot); the foreign subtree, which
        // also differs, is invisible to the scoped run.
        .stdout(predicate::str::contains(
            "added 0 changed 1 removed 0 unchanged 0",
        ));
}

#[test]
fn classify_arch_default_from_config_scopes_to_the_host_subtree() {
    // A committed `[capture].arches` listing the host arch, with no `--arch`,
    // defaults the comparison to the host subtree.
    let dir = TempDir::new().unwrap();
    let key = host_arch();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(&cfg, format!("[capture]\narches = [{key:?}]\n")).unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_one(&base.join(&key), "home", &digest("aa"), b"old");
    write_one(&cur.join(&key), "home", &digest("bb"), b"new");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["classify", "--exit-code", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .code(3)
        .stdout(predicate::str::contains("changed home [viewport=desktop]"))
        .stdout(predicate::str::contains(
            "added 0 changed 1 removed 0 unchanged 0",
        ));
}

#[test]
fn classify_host_arch_not_in_configured_arches_hard_errors_on_stderr() {
    // A config whose arches cannot contain the host, with no `--arch`, fails with
    // the explanatory error rather than scoping to a phantom subtree.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(&cfg, "[capture]\narches = [\"sparc64\"]\n").unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_one(&base.join("sparc64"), "home", &digest("aa"), b"x");
    write_one(&cur.join("sparc64"), "home", &digest("aa"), b"x");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["classify", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("is not in the configured arches"));
}

#[test]
fn arches_command_prints_configured_arches() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(&cfg, "[capture]\narches = [\"x86_64\", \"arm64\"]\n").unwrap();

    // Human: one per line.
    bin()
        .args(["--config"])
        .arg(&cfg)
        .arg("arches")
        .assert()
        .success()
        .stdout(predicate::str::contains("x86_64"))
        .stdout(predicate::str::contains("arm64"));

    // JSON: a single-line array, consumed by the CI matrix.
    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["arches", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"["x86_64","arm64"]"#));
}

#[test]
fn arches_is_empty_array_without_configured_arches() {
    // The reusable workflow's matrix guard branches on the literal "[]"; an
    // unconfigured project must print exactly that (and nothing in human mode)
    // rather than erroring, so the workflow can emit a clear "configure arches"
    // message instead of a cryptic parse failure.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(&cfg, "[comment]\nmarker = \"x\"\n").unwrap();

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["arches", "--format", "json"])
        .assert()
        .success()
        .stdout("[]\n");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .arg("arches")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn gallery_diff_scopes_both_trees_by_arch() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let out = TempDir::new().unwrap();
    write_one(&base.join("arm64"), "home", &digest("aa"), b"old");
    write_one(&cur.join("arm64"), "home", &digest("bb"), b"new");

    bin()
        .args(["gallery", "--arch", "arm64", "--input"])
        .arg(&cur)
        .arg("--baseline")
        .arg(&base)
        .arg("--output")
        .arg(out.path())
        .assert()
        .success();

    // Copied trees drop the arch layer: the diff page is self-contained.
    assert!(out.path().join("baseline/home.png").exists());
    assert!(out.path().join("current/home.png").exists());
}

#[test]
fn missing_arch_subtree_hints_the_layout_on_stderr() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_one(&base.join("arm64"), "home", &digest("aa"), b"x");
    write_one(&cur.join("arm64"), "home", &digest("aa"), b"x");

    bin()
        .args(["classify", "--arch", "x86_64", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        // A layout hint naming the missing key and the arch layer, not a bare
        // "not a directory".
        .stderr(predicate::str::contains("x86_64"))
        .stderr(predicate::str::contains("--arch"));
}

#[test]
fn verify_identical_captures_pass_and_diverging_ones_exit_three() {
    // Two reads of the same capture are byte-identical: the gate passes.
    bin()
        .args(["verify", "--first"])
        .arg(current())
        .arg("--second")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("reproducible:"))
        .stderr(predicate::str::is_empty());

    // Baseline vs current differ: the gate fails with code 3, output on stdout.
    bin()
        .args(["verify", "--first"])
        .arg(baseline())
        .arg("--second")
        .arg(current())
        .assert()
        .code(3)
        .stdout(predicate::str::contains("NOT reproducible:"))
        .stdout(predicate::str::contains("differs about [viewport=desktop]"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn verify_json_contract() {
    bin()
        .args(["verify", "--format", "json", "--first"])
        .arg(baseline())
        .arg("--second")
        .arg(current())
        .assert()
        .code(3)
        .stdout(predicate::str::contains(r#""reproducible":false"#))
        .stdout(predicate::str::contains(r#""kind":"differs""#));
}

#[test]
fn doctor_reports_a_clean_capture_layout() {
    // The fixtures use a `viewport` toggle, so declare it in config; doctor then
    // lists the observed dimension and passes cleanly.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["doctor", "--input"])
        .arg(current())
        .assert()
        .success()
        // The current fixture has three names (about, home, pricing) and four
        // shots (home has two viewport variants).
        .stdout(predicate::str::contains("names: 3"))
        .stdout(predicate::str::contains("home (2 shots)"))
        .stdout(predicate::str::contains("shots: 4"))
        .stdout(predicate::str::contains("viewport [desktop, mobile]"))
        .stdout(predicate::str::contains("ok: capture index is well-formed"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn doctor_exit_code_gate_catches_a_capture_without_an_index() {
    let dir = TempDir::new().unwrap();
    // A capture directory missing its captures.json index.
    std::fs::write(dir.path().join("home.png"), b"oops").unwrap();

    bin()
        .args(["doctor", "--exit-code", "--input"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("captures.json"));
}

#[test]
fn doctor_exit_code_gate_catches_an_undeclared_toggle() {
    // A valid index whose toggle key is not declared in config is a "problem"
    // doctor gates on with --exit-code (output stays on stdout, no error).
    let dir = TempDir::new().unwrap();
    write_capture(
        dir.path(),
        &[(
            "home",
            &[("density", "2x")],
            &digest("aa"),
            "home.png",
            b"a",
        )],
    );

    bin()
        .args(["doctor", "--exit-code", "--input"])
        .arg(dir.path())
        .assert()
        .code(3)
        .stdout(predicate::str::contains("warning:"))
        .stdout(predicate::str::contains("problems found"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn doctor_missing_input_fails_with_clean_stderr() {
    bin()
        .args(["doctor", "--input", "/no/such/dir"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not a directory"));
}

#[test]
fn manifest_baseline_journey_replaces_committed_images() {
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("baseline.json");

    // Produce a digest baseline instead of committing baseline PNGs.
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));

    // Classify a current capture against just that baseline — no baseline images.
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("changed about [viewport=desktop]"))
        .stdout(predicate::str::contains(
            "added 1 changed 1 removed 0 unchanged 2",
        ))
        .stderr(predicate::str::is_empty());
}

/// Write PNGs into `dir` the way a capture step leaves them: files only, with no
/// `captures.json` beside them yet.
fn write_pngs(dir: &Path, shots: &[(&str, &[u8])]) {
    for (relative, bytes) in shots {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// The capture a capture step produces: two screens at two viewports, encoded as
/// `<name>/viewport=<value>.png` so `--toggles-from-path` recovers the dimension.
const CAPTURED: [(&str, &[u8]); 4] = [
    ("home/viewport=desktop.png", b"home-desktop"),
    ("home/viewport=mobile.png", b"home-mobile"),
    ("about/viewport=desktop.png", b"about-desktop"),
    ("about/viewport=mobile.png", b"about-mobile"),
];

#[test]
fn index_authors_a_capture_the_other_commands_consume_unchanged() {
    // The journey a consumer replaces its hand-rolled hashing with: capture PNGs,
    // `index` them, then run the rest of the tool against that index untouched.
    let dir = TempDir::new().unwrap();
    let current = dir.path().join("current");
    write_pngs(&current, &CAPTURED);
    // The dimension the capture encodes in its paths, declared as the gallery
    // control it should become.
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[[toggle]]\nkey = \"viewport\"\nvalues = [\"desktop\", \"mobile\"]\n",
    )
    .unwrap();

    bin()
        .args(["index", "--input"])
        .arg(&current)
        .arg("--toggles-from-path")
        .assert()
        .success()
        .stdout(predicate::str::contains("captures.json"))
        .stdout(predicate::str::contains("4 shots"))
        .stderr(predicate::str::is_empty());

    // The index is a real captures.json: schema, names collapsed onto the screen,
    // the toggle recovered from the path, and image paths relative to the index.
    let index = std::fs::read_to_string(current.join("captures.json")).unwrap();
    assert!(index.contains("\"schema\": 1"), "{index}");
    assert!(index.contains("\"viewport\": \"desktop\""), "{index}");
    assert!(
        index.contains("\"image\": \"home/viewport=mobile.png\""),
        "{index}"
    );

    // Classifying the capture against itself sees no drift at all — the hashes
    // `index` wrote are exactly what classify treats as the content digest.
    bin()
        .args(["classify", "--baseline"])
        .arg(&current)
        .arg("--current")
        .arg(&current)
        .args(["--exit-code"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "added 0 changed 0 removed 0 unchanged 4",
        ));

    // It feeds `manifest` (an image-free baseline) …
    let manifest = dir.path().join("baseline.json");
    bin()
        .args(["manifest", "--input"])
        .arg(&current)
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success();
    let baseline = std::fs::read_to_string(&manifest).unwrap();
    assert!(baseline.contains("\"name\": \"home\""), "{baseline}");
    assert!(!baseline.contains("\"image\""), "{baseline}");

    // … and `gallery`, which copies each indexed image beside the page and turns
    // the recovered toggle into a control.
    let gallery = dir.path().join("public");
    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["gallery", "--input"])
        .arg(&current)
        .arg("--output")
        .arg(&gallery)
        .assert()
        .success();
    assert!(gallery.join("index.html").is_file());
    assert!(gallery.join("home/viewport=mobile.png").is_file());
    let page = std::fs::read_to_string(gallery.join("index.html")).unwrap();
    assert!(
        page.contains("desktop") && page.contains("mobile"),
        "{page}"
    );

    // Doctor accepts the layout with no problems: every toggle `index` recorded is
    // one the project declares.
    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["doctor", "--input"])
        .arg(&current)
        .arg("--exit-code")
        .assert()
        .success()
        .stdout(predicate::str::contains("shots: 4"))
        .stdout(predicate::str::contains("viewport [desktop, mobile]"));
}

#[test]
fn index_makes_two_captures_of_one_build_verify_clean_and_a_changed_png_classify_changed() {
    // The reproducibility gate over indexes this command authored: two captures of
    // the same build hash identically, and a single changed PNG shows up as exactly
    // one `changed` shot once its tree is re-indexed.
    let dir = TempDir::new().unwrap();
    let first = dir.path().join("current");
    let second = dir.path().join("verify");
    write_pngs(&first, &CAPTURED);
    write_pngs(&second, &CAPTURED);

    for capture in [&first, &second] {
        bin()
            .args(["index", "--input"])
            .arg(capture)
            .arg("--toggles-from-path")
            .assert()
            .success();
    }

    bin()
        .args(["verify", "--first"])
        .arg(&first)
        .arg("--second")
        .arg(&second)
        .assert()
        .success()
        .stdout(predicate::str::contains("reproducible: 4 shots"));

    // Re-capture one screen differently, re-index, and the gate flags just it.
    write_pngs(
        &second,
        &[("home/viewport=mobile.png", b"home-mobile-redesigned")],
    );
    bin()
        .args(["index", "--input"])
        .arg(&second)
        .arg("--toggles-from-path")
        .assert()
        .success();
    bin()
        .args(["classify", "--baseline"])
        .arg(&first)
        .arg("--current")
        .arg(&second)
        .arg("--exit-code")
        .assert()
        .code(3)
        .stdout(predicate::str::contains("changed home [viewport=mobile]"))
        .stdout(predicate::str::contains(
            "added 0 changed 1 removed 0 unchanged 3",
        ));
}

#[test]
fn index_writes_the_arch_lane_the_other_commands_read() {
    // A capture lane writes into `<root>/<arch>/`; `index` resolves the arch from
    // `[capture].arches` exactly as classify does, so the two agree on the subtree.
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(&cfg, "[capture]\narches = [\"x86_64\", \"arm64\"]\n").unwrap();
    let current = dir.path().join("current");
    write_pngs(
        &current.join("x86_64"),
        &[("home/desktop.png", b"x86-home")],
    );

    // `wrote <path>` names a file on this host, so it carries the host separator;
    // build the expectation the same way rather than assuming `/`.
    let lane_index = current.join("x86_64").join("captures.json");
    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["index", "--input"])
        .arg(&current)
        .args(["--arch", "x86_64"])
        .assert()
        .success()
        .stdout(predicate::str::contains(lane_index.to_str().unwrap()));
    assert!(lane_index.is_file());
    assert!(!current.join("captures.json").exists());

    // A shot's identity is not host-dependent: the name and image path the lane's
    // index records are `/`-separated whatever separator this host walks with.
    let index = std::fs::read_to_string(&lane_index).unwrap();
    assert!(index.contains("\"name\": \"home/desktop\""), "{index}");
    assert!(index.contains("\"image\": \"home/desktop.png\""), "{index}");

    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["classify", "--baseline"])
        .arg(&current)
        .arg("--current")
        .arg(&current)
        .args(["--arch", "x86_64"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "added 0 changed 0 removed 0 unchanged 1",
        ));

    // The arch lane that captured nothing fails loudly instead of writing an
    // empty index that would later read as "every shot removed".
    bin()
        .args(["--config"])
        .arg(&cfg)
        .args(["index", "--input"])
        .arg(&current)
        .args(["--arch", "arm64"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("arm64"));
}

#[test]
fn index_reports_an_unindexable_capture_on_stderr() {
    let dir = TempDir::new().unwrap();
    let empty = dir.path().join("current");
    std::fs::create_dir_all(&empty).unwrap();

    bin()
        .args(["index", "--input"])
        .arg(&empty)
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("no .png files"));

    // A malformed --toggle is a usage error, like every other KEY=VALUE argument.
    bin()
        .args(["index", "--input"])
        .arg(&empty)
        .args(["--toggle", "theme"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("expected KEY=VALUE"));
}

#[test]
fn classify_include_scopes_an_affected_only_capture_without_hiding_real_drift() {
    let dir = TempDir::new().unwrap();
    let baseline = dir.path().join("baseline");
    let current = dir.path().join("current");
    let manifest = dir.path().join("baseline.json");
    write_capture(
        &baseline,
        &[
            (
                "home",
                &[("project", "a")],
                &digest("aa"),
                "a-home.png",
                b"a-home",
            ),
            (
                "settings",
                &[("project", "a")],
                &digest("bb"),
                "a-settings.png",
                b"a-settings",
            ),
            (
                "home",
                &[("project", "b")],
                &digest("cc"),
                "b-home.png",
                b"b-home",
            ),
        ],
    );
    bin()
        .args(["manifest", "--input"])
        .arg(&baseline)
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success();

    write_capture(
        &current,
        &[
            (
                "home",
                &[("project", "a")],
                &digest("aa"),
                "a-home.png",
                b"a-home",
            ),
            (
                "settings",
                &[("project", "a")],
                &digest("bb"),
                "a-settings.png",
                b"a-settings",
            ),
        ],
    );
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(&current)
        .args(["--include", "project=a", "--exit-code"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "added 0 changed 0 removed 0 unchanged 2",
        ));

    write_capture(
        &current,
        &[(
            "home",
            &[("project", "a")],
            &digest("dd"),
            "a-home.png",
            b"changed",
        )],
    );
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(&current)
        .args(["--include", "project=a", "--exit-code"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("changed home [project=a]"))
        .stdout(predicate::str::contains("removed settings [project=a]"))
        .stdout(predicate::str::contains(
            "added 0 changed 1 removed 1 unchanged 0",
        ))
        .stdout(predicate::str::contains("project=b").not());
}

#[test]
fn classify_include_rejects_malformed_selectors() {
    for (selector, message) in [
        ("project", "expected KEY=VALUE"),
        ("=a", "key and value must not be empty"),
        ("project=", "key and value must not be empty"),
        ("project.name=a", "key must match [A-Za-z0-9_-]"),
    ] {
        bin()
            .args(["classify", "--baseline"])
            .arg(baseline())
            .arg("--current")
            .arg(current())
            .args(["--include", selector])
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains(message));
    }
}

#[test]
fn manifest_writes_pretty_json_index_to_stdout() {
    // The written baseline is a pretty-printed captures.json index: schema +
    // digests, with the image paths stripped (a baseline commits no PNGs).
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schema\": 1"))
        .stdout(predicate::str::contains("\"name\": \"about\""))
        .stdout(predicate::str::contains("\"hash\":"))
        .stdout(predicate::str::contains("\"image\"").not());
}

#[test]
fn classify_requires_exactly_one_baseline_source() {
    // Neither source: usage error.
    bin()
        .args(["classify", "--current"])
        .arg(current())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("baseline"));

    // Both sources: mutually exclusive, usage error.
    bin()
        .args(["classify", "--baseline"])
        .arg(baseline())
        .arg("--baseline-manifest")
        .arg("b.json")
        .arg("--current")
        .arg(current())
        .assert()
        .failure()
        .code(2);
}

#[test]
fn malformed_manifest_fails_with_clean_stderr() {
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("bad.json");
    std::fs::write(&manifest, "{not valid json").unwrap();
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid screenshot layout"));
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
fn scope_reads_stdin_and_gates_the_pre_push_guard() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("screencomp.toml");
    std::fs::write(
        &cfg,
        "[guard]\npaths = [\"src/**/*.rs\", \"playwright/**\"]\n",
    )
    .unwrap();

    // A relevant change on stdin: exit 3 (the hook then runs its capture step).
    bin()
        .args(["scope", "--exit-code", "--config"])
        .arg(&cfg)
        .write_stdin("README.md\nsrc/ui/button.rs\n")
        .assert()
        .code(3)
        .stdout(predicate::str::contains("match src/ui/button.rs"))
        .stdout(predicate::str::contains(
            "1 of 2 changed paths are screenshot-relevant",
        ))
        .stderr(predicate::str::is_empty());

    // No relevant change: exit 0 (the hook passes silently, no capture).
    bin()
        .args(["scope", "--exit-code", "--config"])
        .arg(&cfg)
        .write_stdin("README.md\ndocs/guide.md\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "0 of 2 changed paths are screenshot-relevant",
        ))
        .stderr(predicate::str::is_empty());

    // JSON contract on stdin, single line.
    bin()
        .args(["scope", "--format", "json", "--config"])
        .arg(&cfg)
        .write_stdin("src/lib.rs\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""matched":true"#))
        .stdout(predicate::str::contains(r#""paths":["src/lib.rs"]"#));

    // Empty stdin: no candidates, no match, clean exit.
    bin()
        .args(["scope", "--exit-code", "--config"])
        .arg(&cfg)
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "0 of 0 changed paths are screenshot-relevant",
        ));
}

#[test]
fn scope_auto_discovers_screencomp_toml_from_the_working_dir() {
    // Without --config, a screencomp.toml in the working directory (or an
    // ancestor) is found by walking up — so the pre-push guard fires even when
    // the hook forgot to pass --config. The previous behavior (defaults, empty
    // globs) silently never matched.
    let repo = TempDir::new().unwrap();
    std::fs::write(
        repo.path().join("screencomp.toml"),
        "[guard]\npaths = [\"src/**/*.rs\"]\n",
    )
    .unwrap();
    let sub = repo.path().join("crates/app");
    std::fs::create_dir_all(&sub).unwrap();

    // Run from a SUBDIRECTORY with no --config: discovery walks up to the repo
    // root's screencomp.toml and the relevant path matches (exit 3).
    bin()
        .current_dir(&sub)
        .args(["scope", "--exit-code"])
        .write_stdin("src/ui/button.rs\n")
        .assert()
        .code(3)
        .stdout(predicate::str::contains("match src/ui/button.rs"));

    // In a directory with no discoverable config, scope falls back to defaults
    // (empty globs) and matches nothing — no error, just exit 0.
    let empty = TempDir::new().unwrap();
    bin()
        .current_dir(empty.path())
        .args(["scope", "--exit-code"])
        .write_stdin("src/ui/button.rs\n")
        .assert()
        .success();
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

#[test]
fn init_scaffolds_a_working_setup() {
    let dir = TempDir::new().unwrap();

    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .arg("--arch")
        .arg("auto")
        .assert()
        .success()
        .stdout(predicate::str::contains("created"))
        .stdout(predicate::str::contains("Next steps"))
        // The reusable workflow deploys to the gh-pages branch (peaceiris), so the
        // scaffold must point at that Pages source, not "GitHub Actions".
        .stdout(predicate::str::contains("Deploy from a branch: gh-pages"))
        .stderr(predicate::str::is_empty());

    // The scaffolded config parses and its `[capture].arches` drives the default
    // scoping: feeding it back to `comment` succeeds against an arch-scoped tree.
    // The scaffold was generated for the host arch, so the trees carry that subtree.
    let cfg = dir.path().join("screencomp.toml");
    let scoped = TempDir::new().unwrap();
    let base = scoped.path().join("baseline");
    let cur = scoped.path().join("current");
    write_one(&base.join(host_arch()), "home", &digest("aa"), b"old");
    write_one(&cur.join(host_arch()), "home", &digest("bb"), b"new");
    bin()
        .args(["comment", "--config"])
        .arg(&cfg)
        .arg("--baseline")
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .success()
        .stdout(predicate::str::contains("<!-- screencomp -->"));

    let workflow =
        std::fs::read_to_string(dir.path().join(".github/workflows/visual-docs.yml")).unwrap();
    // The scaffold opts into the strict gate explicitly.
    assert!(workflow.contains("fail-on-drift: true"), "{workflow}");
    // It forwards the gh-pages maintenance triggers so the reusable workflow's
    // cleanup-preview (on a closed PR) and prune-history (on schedule) can fire —
    // without these the published branch grows without bound.
    assert!(workflow.contains("closed]"), "{workflow}");
    assert!(
        workflow.contains("schedule:") && workflow.contains("cron:"),
        "{workflow}"
    );
    // Maintenance is on by default and the knob is visible in the consumer's own
    // file (so the opt-out is discoverable), mirroring the explicit strict gate.
    assert!(
        workflow.contains("gh-pages-maintenance: true"),
        "{workflow}"
    );
    // The arch list lives in [capture].arches; CI fans out a lane per arch, so the
    // caller carries no per-arch input.
    assert!(!workflow.contains("runs-on:"), "{workflow}");
    assert!(!workflow.contains("platform:"), "{workflow}");
    assert!(dir.path().join(".gitignore").exists());

    // The strict scaffold also drops the local pre-push guard, executable and with
    // the robust scope-exit handling. The hook detects the host arch at runtime
    // (rather than baking it) so the same committed hook is correct everywhere.
    let hook_path = dir.path().join(".githooks/pre-push");
    let hook = std::fs::read_to_string(&hook_path).unwrap();
    assert!(hook.contains("uname -m"), "{hook}");
    assert!(hook.contains("ARCH=\"arm64\""), "{hook}");
    assert!(hook.contains("ARCH=\"x86_64\""), "{hook}");
    assert!(hook.contains("DOCKER_PLATFORM=\"linux/arm64\""), "{hook}");
    assert!(hook.contains("DOCKER_PLATFORM=\"linux/amd64\""), "{hook}");
    // The committed baseline is the new JSON index, not a `.sha256` text manifest.
    assert!(
        hook.contains("MANIFEST=\"shots/baseline/${ARCH}.json\""),
        "{hook}"
    );
    // The classify call no longer passes --arch (it defaults from the config).
    assert!(
        hook.contains(
            "screencomp classify --baseline-manifest \"$MANIFEST\" --current \"$CURRENT\" --exit-code"
        ),
        "{hook}"
    );
    // Only exit 3 means "relevant"; an errored scope check skips, not captures.
    assert!(hook.contains("scope_status"), "{hook}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&hook_path).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "hook must be executable: {mode:o}");
    }
}

#[test]
fn comment_manifest_mode_sources_before_from_a_separate_baseline_url() {
    // A baseline manifest strips image paths on write, so manifest mode commits no
    // "Before" PNG. But pointing --baseline-url at a separate canonical gallery
    // (which hosts the same shot at the same relative path) still yields a real
    // before/after diff: "Before" from the baseline base, "After" from --current-url.
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("baseline.json");
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success();

    let out = dir.path().join("comment.md");
    bin()
        .args(["comment", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .arg("--baseline-url")
        .arg("https://example.test/main")
        .arg("--current-url")
        .arg("https://example.test/pr/4")
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let md = std::fs::read_to_string(&out).expect("comment file");
    // The changed shot (about [viewport=desktop]) shows a real before/after table,
    // before sourced from the canonical gallery and after from the PR gallery.
    assert!(md.contains("| Before | After |"), "{md}");
    assert!(
        md.contains("src=\"https://example.test/main/about-desktop.png\""),
        "{md}"
    );
    assert!(
        md.contains("src=\"https://example.test/pr/4/about-desktop.png\""),
        "{md}"
    );
}

#[test]
fn comment_urls_resolve_to_real_gallery_files() {
    // The regression guard for "gallery and comment disagree on URL layout":
    // build a plain gallery, then assert every inline image the comment emits
    // (via --current-url) resolves to a file the gallery actually wrote.
    let dir = TempDir::new().unwrap();
    let gallery_dir = dir.path().join("site");
    bin()
        .args(["gallery", "--input"])
        .arg(current())
        .arg("--output")
        .arg(&gallery_dir)
        .assert()
        .success();

    let manifest = dir.path().join("baseline.json");
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success();

    let base = "https://example.test/site";
    let out = dir.path().join("comment.md");
    bin()
        .args(["comment", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .arg("--current-url")
        .arg(base)
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let md = std::fs::read_to_string(&out).expect("comment file");
    let prefix = format!("{base}/");
    let mut checked = 0;
    for piece in md.split("src=\"").skip(1) {
        let url = &piece[..piece.find('"').expect("closing quote")];
        if let Some(rel) = url.strip_prefix(&prefix) {
            assert!(
                gallery_dir.join(rel).exists(),
                "comment references {rel}, but the gallery never wrote it (layouts disagree)"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "expected at least one inline image to verify: {md}"
    );
}

/// Whether `git` is installed, so git-dependent assertions can skip cleanly in a
/// minimal environment rather than fail.
fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
}

/// Run `git -C <dir> <args>`, asserting success.
fn git(dir: &TempDir, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

#[test]
fn init_hook_survives_proxies_and_matches_ci_clean_install() {
    // The scaffolded capture must work in containerized/proxied dev environments
    // and match CI's fresh checkout — the gaps that cost the most setup time.
    let dir = TempDir::new().unwrap();
    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .args(["--arch", "auto"])
        .assert()
        .success();
    let hook = std::fs::read_to_string(dir.path().join(".githooks/pre-push")).unwrap();
    // A host directory masks node_modules, so `npm ci` installs inside the
    // container without churning the bind-mounted tree.
    assert!(hook.contains(":/work/node_modules\""), "{hook}");
    // Host CA pass-through so a TLS-intercepting proxy doesn't break `npm ci`.
    assert!(hook.contains("NODE_EXTRA_CA_CERTS"), "{hook}");
    assert!(hook.contains("ca_args"), "{hook}");
    // A missing CLI is loud and opt-in-strict, never a silent skip.
    assert!(hook.contains("SCREENCOMP_GUARD_REQUIRE"), "{hook}");
    assert!(hook.contains("cannot run"), "{hook}");
}

#[test]
fn init_enable_hook_wires_the_git_hooks_path() {
    if !git_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    git(&dir, &["init", "-q"]);

    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .args(["--arch", "auto", "--enable-hook"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Enabled the local pre-push guard"));

    // Git is now pointed at the committed hooks directory.
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), ".githooks");
}

#[test]
fn init_enable_hook_outside_a_repo_reports_without_failing() {
    // The scaffold still succeeds; only the enable step is skipped, with guidance.
    let dir = TempDir::new().unwrap();
    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .args(["--arch", "auto", "--enable-hook", "--format", "json"])
        .assert()
        .success()
        // A bare temp dir is not a git repo, so enabling fails (or git is absent).
        .stdout(
            predicate::str::contains(r#""hook_enabled":"failed""#)
                .or(predicate::str::contains(r#""hook_enabled":"git-unavailable""#)),
        );
}

#[test]
fn doctor_env_flags_a_scaffolded_but_unenabled_guard() {
    // The inert-guard gap: init drops .githooks/pre-push but core.hooksPath is
    // never set, so the repo looks protected while nothing runs.
    let dir = TempDir::new().unwrap();
    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .args(["--arch", "auto"])
        .assert()
        .success();

    bin()
        .args(["doctor", "--env", "--exit-code", "--dir"])
        .arg(dir.path())
        .assert()
        .code(3)
        .stdout(predicate::str::contains("PRESENT BUT NOT ENABLED"))
        .stdout(predicate::str::contains("problems found"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn doctor_env_flags_a_workflow_version_skew() {
    let dir = TempDir::new().unwrap();
    let workflows = dir.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(
        workflows.join("visual-docs.yml"),
        "jobs:\n  visual-docs:\n    uses: nickderobertis/screencomp/.github/workflows/\
         visual-docs-reusable.yml@v9.9.9\n",
    )
    .unwrap();

    bin()
        .args(["doctor", "--env", "--exit-code", "--dir"])
        .arg(dir.path())
        .assert()
        .code(3)
        .stdout(predicate::str::contains("SKEW"))
        .stdout(predicate::str::contains("v9.9.9"));
}

#[test]
fn doctor_env_clean_directory_reports_ready_json() {
    let dir = TempDir::new().unwrap();
    bin()
        .args(["doctor", "--env", "--dir"])
        .arg(dir.path())
        .args(["--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""pre_push_guard":"not-scaffolded""#,
        ))
        .stdout(predicate::str::contains(r#""workflow_pin":"no-workflow""#))
        .stdout(predicate::str::contains(r#""ok":true"#))
        .stderr(predicate::str::is_empty());
}

#[test]
fn doctor_env_reports_an_enabled_guard_in_step() {
    if !git_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    git(&dir, &["init", "-q"]);
    bin()
        .args(["init", "--dir"])
        .arg(dir.path())
        .args(["--arch", "auto", "--enable-hook"])
        .assert()
        .success();

    // Guard enabled and the scaffolded workflow pins this very CLI version, so the
    // environment is ready (Docker is advisory and never fails the preflight).
    bin()
        .args(["doctor", "--env", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("pre-push guard: enabled"))
        .stdout(predicate::str::contains("matches this CLI"))
        .stdout(predicate::str::contains("ok: environment ready"));
}
