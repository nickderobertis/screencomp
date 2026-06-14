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
    // Without a gallery URL the comment is a path listing, not inline images.
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
    assert!(md.contains("src=\"https://example.test/pr/12/baseline/desktop/about.png\""));
    assert!(md.contains("src=\"https://example.test/pr/12/current/desktop/pricing.png\""));
    assert!(md.contains("width=\"380\""));
}

#[test]
fn comment_manifest_mode_embeds_current_only_from_gallery_url() {
    // The headline image-free feature: with a digest-manifest baseline there are
    // no baseline PNGs to host, so a `--gallery-url` (a plain gallery of the
    // current shots) must embed only "After" images at `<URL>/<project>/<name>.png`
    // — never a `baseline/` URL that would 404 in the rendered comment.
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("baseline.sha256");
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
        md.contains("src=\"https://example.test/site/desktop/about.png\""),
        "{md}"
    );
    assert!(!md.contains("/baseline/"), "{md}");
    assert!(!md.contains("/current/"), "{md}");
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

/// Host platform key, mirroring `commands::platform::host_key`.
fn host_key() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

fn write_shot(root: &std::path::Path, platform: &str, project: &str, name: &str, bytes: &[u8]) {
    let dir = root.join(platform).join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.png")), bytes).unwrap();
}

#[test]
fn classify_platform_auto_compares_only_the_host_subtree() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let key = host_key();

    // Host subtree changes; a foreign subtree differs too but must be ignored.
    write_shot(&base, &key, "desktop", "home", b"old");
    write_shot(&cur, &key, "desktop", "home", b"new");
    write_shot(&base, "other-arch", "desktop", "home", b"a");
    write_shot(&cur, "other-arch", "desktop", "home", b"b");

    bin()
        .args(["classify", "--platform", "auto", "--exit-code", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .code(3)
        .stdout(predicate::str::contains("changed desktop/home"))
        // Only the host subtree is compared (1 shot); the foreign subtree, which
        // also differs, is invisible to the scoped run.
        .stdout(predicate::str::contains(
            "added 0 changed 1 removed 0 unchanged 0",
        ));
}

#[test]
fn gallery_diff_scopes_both_trees_by_platform() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    let out = TempDir::new().unwrap();
    write_shot(&base, "linux-arm64", "desktop", "home", b"old");
    write_shot(&cur, "linux-arm64", "desktop", "home", b"new");

    bin()
        .args(["gallery", "--platform", "linux-arm64", "--input"])
        .arg(&cur)
        .arg("--baseline")
        .arg(&base)
        .arg("--output")
        .arg(out.path())
        .assert()
        .success();

    // Copied trees drop the platform layer: the diff page is self-contained.
    assert!(out.path().join("baseline/desktop/home.png").exists());
    assert!(out.path().join("current/desktop/home.png").exists());
}

#[test]
fn missing_platform_subtree_hints_the_layout_on_stderr() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("baseline");
    let cur = dir.path().join("current");
    write_shot(&base, "linux-arm64", "desktop", "home", b"x");
    write_shot(&cur, "linux-arm64", "desktop", "home", b"x");

    bin()
        .args(["classify", "--platform", "windows-x86_64", "--baseline"])
        .arg(&base)
        .arg("--current")
        .arg(&cur)
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        // A layout hint naming the missing key and the platform layer, not a bare
        // "not a directory".
        .stderr(predicate::str::contains("windows-x86_64"))
        .stderr(predicate::str::contains("--platform"));
}

#[test]
fn verify_identical_captures_pass_and_diverging_ones_exit_three() {
    // Two reads of the same tree are byte-identical: the gate passes.
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
        .stdout(predicate::str::contains("differs desktop/about"))
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
    bin()
        .args(["doctor", "--input"])
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("desktop (3 shots)"))
        .stdout(predicate::str::contains("shots: 4"))
        .stdout(predicate::str::contains("ok: layout matches"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn doctor_exit_code_gate_catches_a_misplaced_capture() {
    let dir = TempDir::new().unwrap();
    // A capture written to the root instead of <project>/<name>.png.
    std::fs::write(dir.path().join("home.png"), b"oops").unwrap();

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
    let manifest = dir.path().join("baseline.sha256");

    // Produce a digest manifest instead of committing baseline PNGs.
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .arg("--output")
        .arg(&manifest)
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));

    // Classify a current capture against just that manifest — no baseline images.
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("changed desktop/about"))
        .stdout(predicate::str::contains(
            "added 1 changed 1 removed 0 unchanged 2",
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn manifest_writes_sha256sum_style_to_stdout() {
    bin()
        .args(["manifest", "--input"])
        .arg(baseline())
        .assert()
        .success()
        .stdout(predicate::str::contains("  desktop/about.png"))
        .stdout(predicate::str::is_match(r"^[0-9a-f]{64}  ").unwrap());
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
        .arg("b.sha256")
        .arg("--current")
        .arg(current())
        .assert()
        .failure()
        .code(2);
}

#[test]
fn malformed_manifest_fails_with_clean_stderr() {
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("bad.sha256");
    std::fs::write(&manifest, "deadbeef  desktop/home.png\n").unwrap();
    bin()
        .args(["classify", "--baseline-manifest"])
        .arg(&manifest)
        .arg("--current")
        .arg(current())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid screenshot layout"))
        .stderr(predicate::str::contains("line 1"));
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
        .arg("--platform")
        .arg("linux-x86_64")
        .assert()
        .success()
        .stdout(predicate::str::contains("created"))
        .stdout(predicate::str::contains("Next steps"))
        .stderr(predicate::str::is_empty());

    // The scaffolded config parses: feeding it back to `comment` succeeds.
    let cfg = dir.path().join("screencomp.toml");
    bin()
        .args(["comment", "--config"])
        .arg(&cfg)
        .arg("--baseline")
        .arg(baseline())
        .arg("--current")
        .arg(current())
        .assert()
        .success()
        .stdout(predicate::str::contains("<!-- screencomp -->"));

    assert!(
        dir.path()
            .join(".github/workflows/visual-docs.yml")
            .exists()
    );
    assert!(dir.path().join(".gitignore").exists());
}

#[test]
fn comment_manifest_mode_before_after_from_separate_urls() {
    // Manifest mode with explicit --baseline-url/--current-url restores a real
    // before/after diff: "Before" from a canonical gallery, "After" from the PR
    // one. This is the decoupling that makes manifest-mode comments usable.
    let dir = TempDir::new().unwrap();
    let manifest = dir.path().join("baseline.sha256");
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
    assert!(md.contains("| Before | After |"), "{md}");
    assert!(
        md.contains("src=\"https://example.test/main/desktop/about.png\""),
        "{md}"
    );
    assert!(
        md.contains("src=\"https://example.test/pr/4/desktop/about.png\""),
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

    let manifest = dir.path().join("baseline.sha256");
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
