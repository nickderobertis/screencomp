//! Contract tests over the shipped visual-docs surfaces: the composite actions,
//! the reusable workflow, the scripts they run, and every documented or synced
//! copy of the capture invocation (README, `examples/`, `demo/`, `sync-demo.yml`).
//!
//! They live in the `screencomp` crate, so its `test` target (and the coverage
//! aggregate) runs them with the rest of the suite, and they are also the
//! `visual-docs-actions` project's `test` target (`cargo nextest run -p
//! screencomp --test actions`), so a change confined to those files reruns them
//! without reaching the crate's own suite.
// llmlint: ignore-file[tests_mirror_real_usage] The visual-docs acceptance test intentionally extracts and composes the shipped action's fetch/build blocks: GitHub exposes no offline composite-action runner, and executing these exact blocks together is the requested CI-path boundary without remote side effects.

use std::path::{Path, PathBuf};

use clap::Parser as _;
use screencomp::{AppError, Cli, run};
use tempfile::TempDir;

// The subprocess helpers and fixtures serve only the `#[cfg(unix)]` tests, which
// run the shipped shell scripts.
#[cfg(unix)]
mod common;
#[cfg(unix)]
use common::command;

#[cfg(unix)]
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[cfg(unix)]
fn baseline() -> PathBuf {
    fixtures().join("baseline")
}

#[cfg(unix)]
fn current() -> PathBuf {
    fixtures().join("current")
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
    // visual-docs, visual-docs-pages, visual-docs-aggregate, gh-pages-maintenance)
    // by the floating major tag `@v0`, which each release advances to itself
    // (release.yml). `uses:` can't interpolate a ref, so an exact `@vX.Y.Z` pin
    // would silently go stale every release and a brand-new action can't be
    // referenced before it ships — `@v0` sidesteps both. Guard against a
    // regression back to exact pins.
    let reusable = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflows/visual-docs-reusable.yml"),
    )
    .unwrap();

    let mut actions = std::collections::BTreeSet::new();
    for line in reusable.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((_, after)) = line.split_once("uses: nickderobertis/screencomp") else {
            continue;
        };
        let Some((action, ref_part)) = after.split_once('@') else {
            continue;
        };
        let pin: String = ref_part
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        actions.insert(action.to_owned());
        assert_eq!(
            pin, "v0",
            "internal action ref `@{pin}` should float on `@v0`, not an exact pin \
             (which goes stale every release): {line}"
        );
    }
    for action in [
        "",
        "/visual-docs",
        "/visual-docs-pages",
        "/visual-docs-aggregate",
        "/gh-pages-maintenance",
    ] {
        assert!(
            actions.contains(action),
            "the reusable workflow no longer references screencomp{action}@v0; found {actions:?}"
        );
    }
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
            !command("bash")
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
        command("bash")
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
        command("bash")
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
        command("bash")
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

    let mut missing = command("bash");
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
    let mut invalid = command("bash");
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

    let mut external = command("bash");
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
            command("git")
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
    let fetch = command("bash")
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
    let missing_index = command("bash")
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
            command("git")
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
    let branch_absent = command("bash")
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
        let result = command("bash")
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
    let invalid_aggregate = command("bash")
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
        justfile.contains("\nalias gate := check\n"),
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
        command(&binary)
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
            command("git")
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
    let fetched = command("bash")
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
    let built = command("bash")
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
            command("git")
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
        command("bash")
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
        command("bash")
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
    let result = command("bash")
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
            let staged = command("bash")
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
            let unpack = command("bash")
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
    let output = command("bash")
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
        let rejected = command("bash")
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
            command("git")
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
        let result = command("bash")
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
            command("git")
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
        let mut bash = command("bash");
        bash.arg("-c")
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
            bash.env(key, value);
        }
        let result = bash.output().unwrap();
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
    let out = command("git")
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
fn the_workflow_and_documented_copies_run_as_the_host_user() {
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

/// Drives the root action's real "Install screencomp" step against a local
/// server standing in for GitHub's release downloads and REST API.
#[cfg(unix)]
mod install_action {
    use std::collections::HashMap;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::process::CommandExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use sha2::{Digest as _, Sha256};
    use tempfile::TempDir;

    use super::{action_step_script, command, path_str, repo_file};

    const TAG: &str = "v9.9.9";
    /// How long one install may run before its test fails, so an installer
    /// without request timeouts fails rather than hangs the suite.
    const INSTALL_DEADLINE: Duration = Duration::from_secs(60);
    /// The attempt limit the action ships with.
    const MAX_ATTEMPTS: usize = 8;
    /// The test backoff, in milliseconds: base 20 doubling to a cap of 80.
    const SCALED_DELAYS_MS: [u64; 7] = [20, 40, 80, 80, 80, 80, 80];

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    enum Op {
        Lookup,
        Archive,
        Checksum,
    }

    const OPS: [Op; 3] = [Op::Lookup, Op::Archive, Op::Checksum];

    #[derive(Clone, Copy, Debug)]
    enum Reply {
        Valid,
        Status(u16),
        /// Accept the request and never answer it, so the client times out.
        Hang,
        /// Read the request, then close the connection without answering.
        Drop,
        WrongChecksum,
        /// Promise more body than is sent, then close the connection.
        Truncated,
        /// A 200 lookup response that carries no tag.
        NoTag,
        /// A 200 lookup response that carries this tag.
        Tag(&'static str),
        /// A 200 lookup response with exactly this body.
        Raw(&'static str),
    }

    const TRANSIENT: [Reply; 4] = [
        Reply::Status(504),
        Reply::Hang,
        Reply::Drop,
        Reply::Truncated,
    ];

    #[derive(Clone, Debug)]
    struct Hit {
        op: Op,
        at: Instant,
        authorization: Option<String>,
    }

    type Plan = HashMap<Op, Vec<Reply>>;

    struct Server {
        url: String,
        hits: Arc<Mutex<Vec<Hit>>>,
    }

    /// A tar.gz of the real binary, built once per test process.
    fn archive() -> &'static [u8] {
        static ARCHIVE: OnceLock<Vec<u8>> = OnceLock::new();
        ARCHIVE.get_or_init(|| {
            let dir = TempDir::new().unwrap();
            std::fs::copy(
                env!("CARGO_BIN_EXE_screencomp"),
                dir.path().join("screencomp"),
            )
            .unwrap();
            let tar = dir.path().join("out.tar.gz");
            let status = command("tar")
                .arg("-czf")
                .arg(&tar)
                .arg("-C")
                .arg(dir.path())
                .arg("screencomp")
                .status()
                .unwrap();
            assert!(status.success());
            std::fs::read(tar).unwrap()
        })
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        // A client that gave up mid-response is not a server failure.
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    }

    fn classify(path: &str) -> Option<Op> {
        if path.ends_with("/releases/latest") {
            Some(Op::Lookup)
        } else if path.ends_with(".tar.gz") {
            Some(Op::Archive)
        } else if path.ends_with(".sha256") {
            Some(Op::Checksum)
        } else {
            None
        }
    }

    fn valid_body(op: Op, path: &str) -> Vec<u8> {
        match op {
            // The release body carries an escaped decoy that must not be read
            // as the tag.
            Op::Lookup => format!(
                r#"{{"body":"see \"tag_name\": \"v0.0.1\"","tag_name":"{TAG}","draft":false}}"#
            )
            .into_bytes(),
            Op::Archive => archive().to_vec(),
            Op::Checksum => checksum_line(&format!("{:x}", Sha256::digest(archive())), path),
        }
    }

    /// A `sha256sum` line naming the archive the requested checksum belongs to.
    fn checksum_line(digest: &str, path: &str) -> Vec<u8> {
        let file = path.rsplit('/').next().unwrap();
        let stem = file.strip_suffix(".sha256").unwrap();
        format!("{digest}  {stem}.tar.gz\n").into_bytes()
    }

    fn serve(mut stream: TcpStream, plan: &Plan, hits: &Mutex<Vec<Hit>>) {
        let request = read_request(&mut stream);
        let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
        let Some(op) = classify(&path) else {
            respond(&mut stream, 400, b"unexpected path");
            return;
        };
        let authorization = request
            .lines()
            .find_map(|line| {
                line.strip_prefix("authorization: ")
                    .or(line.strip_prefix("Authorization: "))
            })
            .map(str::to_owned);
        let attempt = {
            let mut hits = hits.lock().unwrap();
            hits.push(Hit {
                op,
                at: Instant::now(),
                authorization,
            });
            hits.iter().filter(|hit| hit.op == op).count() - 1
        };
        let replies = plan.get(&op).map_or(&[Reply::Valid][..], Vec::as_slice);
        match replies[attempt.min(replies.len() - 1)] {
            Reply::Valid => respond(&mut stream, 200, &valid_body(op, &path)),
            Reply::Status(status) => respond(&mut stream, status, b"failure page"),
            Reply::Hang => {
                // Hold the connection until the client hits its timeout.
                let mut sink = [0_u8; 64];
                while matches!(stream.read(&mut sink), Ok(read) if read > 0) {}
            }
            Reply::Drop => drop(stream),
            Reply::WrongChecksum => {
                let body = checksum_line(&"0".repeat(64), &path);
                respond(&mut stream, 200, &body);
            }
            Reply::Truncated => {
                let head =
                    "HTTP/1.1 200 X\r\nContent-Length: 1024\r\nConnection: close\r\n\r\npartial";
                let _ = stream.write_all(head.as_bytes());
            }
            Reply::NoTag => respond(&mut stream, 200, b"{}"),
            Reply::Raw(body) => respond(&mut stream, 200, body.as_bytes()),
            Reply::Tag(tag) => {
                let body = format!(r#"{{"tag_name":"{tag}"}}"#);
                respond(&mut stream, 200, body.as_bytes());
            }
        }
    }

    fn start_server(plan: Plan) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(Vec::new()));
        let plan = Arc::new(plan);
        let thread_hits = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let plan = Arc::clone(&plan);
                let hits = Arc::clone(&thread_hits);
                std::thread::spawn(move || serve(stream, &plan, &hits));
            }
        });
        Server { url, hits }
    }

    struct Install {
        _work: TempDir,
        server_url: String,
        hits: Vec<Hit>,
        success: bool,
        stdout: String,
        stderr: String,
        elapsed: Duration,
        path_file: std::path::PathBuf,
        runner_temp: std::path::PathBuf,
        sleeps: Option<String>,
    }

    impl Install {
        fn attempts(&self, op: Op) -> Vec<Instant> {
            self.hits
                .iter()
                .filter(|hit| hit.op == op)
                .map(|hit| hit.at)
                .collect()
        }

        fn output(&self) -> String {
            format!("stdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
        }

        fn assert_nothing_installed(&self) {
            assert!(!self.path_file.exists(), "{}", self.output());
            assert!(
                !self.runner_temp.join("screencomp-bin/screencomp").exists(),
                "{}",
                self.output()
            );
        }

        /// What the failure message must name for `op`.
        fn expected_name_and_url(&self, op: Op) -> (String, String) {
            let url = &self.server_url;
            match op {
                Op::Lookup => (
                    "latest release of owner/repo".to_owned(),
                    format!("{url}/api/repos/owner/repo/releases/latest"),
                ),
                Op::Archive => (
                    format!("release archive screencomp-{TAG}-"),
                    format!("{url}/owner/repo/releases/download/{TAG}/screencomp-{TAG}-"),
                ),
                Op::Checksum => (
                    format!("release checksum screencomp-{TAG}-"),
                    format!("{url}/owner/repo/releases/download/{TAG}/screencomp-{TAG}-"),
                ),
            }
        }
    }

    #[derive(Clone, Copy)]
    enum Timing {
        /// Millisecond backoff and a two-second attempt timeout.
        Scaled,
        /// The shipped schedule, with `sleep` recording instead of waiting.
        ShippedWithRecordedSleep,
    }

    fn install(plan: Plan, version: &str, timing: Timing) -> Install {
        install_with(plan, version, timing, |_| {})
    }

    /// `install`, with `adjust` applied last to the installer's command.
    fn install_with(
        plan: Plan,
        version: &str,
        timing: Timing,
        adjust: impl FnOnce(&mut Command),
    ) -> Install {
        let server = start_server(plan);
        // Built before the installer starts: building it inside the first request
        // would count against that request's short client timeout.
        archive();
        let work = TempDir::new().unwrap();
        let runner_temp = work.path().join("runner");
        std::fs::create_dir(&runner_temp).unwrap();
        let path_file = work.path().join("github-path");
        let script = action_step_script(&repo_file("action.yml"), "Install screencomp");

        let mut cmd = command("bash");
        cmd.arg("-c")
            .arg(script)
            .current_dir(work.path())
            .env("SCREENCOMP_VERSION", version)
            .env("SCREENCOMP_METHOD", "download")
            .env("SCREENCOMP_REPO", "owner/repo")
            .env("SCREENCOMP_ACTION_PATH", work.path())
            .env("GH_TOKEN", "test-token")
            .env("RUNNER_TEMP", &runner_temp)
            .env("GITHUB_PATH", &path_file)
            .env("SCREENCOMP_INSTALL_GITHUB_URL", &server.url)
            .env(
                "SCREENCOMP_INSTALL_GITHUB_API_URL",
                format!("{}/api", server.url),
            );
        let sleep_log = work.path().join("sleeps");
        match timing {
            Timing::Scaled => {
                cmd.env("SCREENCOMP_INSTALL_ATTEMPT_TIMEOUT", "2")
                    .env("SCREENCOMP_INSTALL_RETRY_BASE_MS", "20")
                    .env("SCREENCOMP_INSTALL_RETRY_CAP_MS", "80");
            }
            Timing::ShippedWithRecordedSleep => {
                let bin = work.path().join("bin");
                std::fs::create_dir(&bin).unwrap();
                let stub = bin.join("sleep");
                std::fs::write(
                    &stub,
                    format!("#!/bin/sh\necho \"$1\" >>'{}'\n", path_str(&sleep_log)),
                )
                .unwrap();
                let made_executable = command("chmod").arg("+x").arg(&stub).status().unwrap();
                assert!(made_executable.success());
                let path = std::env::var("PATH").unwrap();
                cmd.env("PATH", format!("{}:{path}", path_str(&bin)));
            }
        }

        adjust(&mut cmd);

        // Output goes to files, not pipes: a surviving curl would otherwise hold
        // the pipes open past the deadline.
        let stdout_path = work.path().join("stdout");
        let stderr_path = work.path().join("stderr");
        let (status, elapsed) = run_to_deadline(cmd, &stdout_path, &stderr_path);
        let hits = server.hits.lock().unwrap().clone();
        Install {
            sleeps: std::fs::read_to_string(&sleep_log).ok(),
            server_url: server.url,
            hits,
            success: status.success(),
            stdout: std::fs::read_to_string(&stdout_path).unwrap(),
            stderr: std::fs::read_to_string(&stderr_path).unwrap(),
            elapsed,
            path_file,
            runner_temp,
            _work: work,
        }
    }

    /// Run `cmd` with its output in the given files, killing it at the deadline.
    fn run_to_deadline(
        mut cmd: Command,
        stdout_path: &Path,
        stderr_path: &Path,
    ) -> (std::process::ExitStatus, Duration) {
        cmd.stdout(std::fs::File::create(stdout_path).unwrap())
            .stderr(std::fs::File::create(stderr_path).unwrap())
            .process_group(0);
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return (status, started.elapsed());
            }
            if started.elapsed() > INSTALL_DEADLINE {
                let group = format!("-{}", child.id());
                let _ = command("kill").args(["-KILL", "--", &group]).status();
                let _ = child.wait();
                panic!(
                    "script had not exited by the {INSTALL_DEADLINE:?} deadline; stderr so far:\n{}",
                    std::fs::read_to_string(stderr_path).unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The tag-lookup path only runs for `latest`, so only it installs `latest`.
    fn version_for(op: Op) -> &'static str {
        if op == Op::Lookup { "latest" } else { TAG }
    }

    fn plan_for(op: Op, replies: Vec<Reply>) -> Plan {
        HashMap::from([(op, replies)])
    }

    /// A `curl` on the PATH that always exits with one code and logs each call,
    /// for transport failures a local server cannot provoke (DNS, TLS, proxy).
    struct CurlStub {
        dir: TempDir,
        log: PathBuf,
    }

    impl CurlStub {
        fn exiting_with(code: u8) -> Self {
            let dir = TempDir::new().unwrap();
            let log = dir.path().join("calls");
            let stub = dir.path().join("curl");
            std::fs::write(
                &stub,
                format!(
                    "#!/bin/sh\ncat >/dev/null\necho \"$*\" >>'{}'\nexit {code}\n",
                    path_str(&log)
                ),
            )
            .unwrap();
            let made_executable = command("chmod").arg("+x").arg(&stub).status().unwrap();
            assert!(made_executable.success());
            Self { dir, log }
        }

        fn install(&self) -> Install {
            self.install_adjusted(TAG, |_| {})
        }

        fn install_adjusted(&self, version: &str, adjust: impl FnOnce(&mut Command)) -> Install {
            let path = std::env::var("PATH").unwrap();
            let path = format!("{}:{path}", path_str(self.dir.path()));
            install_with(Plan::new(), version, Timing::Scaled, |cmd| {
                cmd.env("PATH", path);
                adjust(cmd);
            })
        }

        /// The argument list of each call, oldest first.
        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    /// Run `check` for every operation and reply on its own thread: the
    /// timeout cases each spend whole seconds waiting.
    fn for_each_case(replies: &[Reply], check: impl Fn(Op, Reply) + Sync) {
        std::thread::scope(|scope| {
            for op in OPS {
                for &reply in replies {
                    let check = &check;
                    scope.spawn(move || check(op, reply));
                }
            }
        });
    }

    #[test]
    fn transient_failures_are_retried_until_a_valid_answer_installs_a_working_binary() {
        for_each_case(&TRANSIENT, |op, reply| {
            let run = install(
                plan_for(op, vec![reply, reply, Reply::Valid]),
                version_for(op),
                Timing::Scaled,
            );
            assert!(run.success, "{op:?}/{reply:?}: {}", run.output());
            assert_eq!(run.attempts(op).len(), 3, "{op:?}/{reply:?}");

            let bindir = std::fs::read_to_string(&run.path_file).unwrap();
            let binary = Path::new(bindir.trim()).join("screencomp");
            let version = command(&binary).arg("--version").output().unwrap();
            assert!(version.status.success(), "{op:?}/{reply:?}");
            assert!(
                String::from_utf8_lossy(&version.stdout).starts_with("screencomp "),
                "{op:?}/{reply:?}"
            );
        });
    }

    #[test]
    fn the_lookup_alone_carries_the_token() {
        let run = install(Plan::new(), "latest", Timing::Scaled);
        assert!(run.success, "{}", run.output());
        for hit in &run.hits {
            let expected = (hit.op == Op::Lookup).then(|| "Bearer test-token".to_owned());
            assert_eq!(hit.authorization, expected, "{:?}", hit.op);
        }
        assert!(!run.output().contains("test-token"), "{}", run.output());
    }

    #[test]
    fn persistent_transient_failure_stops_after_bounded_growing_attempts() {
        for_each_case(&TRANSIENT, |op, reply| {
            let run = install(plan_for(op, vec![reply]), version_for(op), Timing::Scaled);
            assert!(!run.success, "{op:?}/{reply:?}: {}", run.output());
            run.assert_nothing_installed();

            let attempts = run.attempts(op);
            assert_eq!(attempts.len(), MAX_ATTEMPTS, "{op:?}/{reply:?}");
            let backoff: u64 = SCALED_DELAYS_MS.iter().sum();
            let limit =
                Duration::from_millis(backoff) + Duration::from_secs(MAX_ATTEMPTS as u64 + 30);
            assert!(run.elapsed < limit, "{op:?}/{reply:?}: {:?}", run.elapsed);

            let (name, url) = run.expected_name_and_url(op);
            let failure = run.stderr.lines().last().unwrap();
            assert!(
                failure.contains(&format!("failed after {MAX_ATTEMPTS} attempts"))
                    && failure.contains(&name)
                    && failure.contains(&url),
                "{op:?}/{reply:?}: {failure}"
            );

            for (index, delay_ms) in SCALED_DELAYS_MS.iter().enumerate() {
                let gap = attempts[index + 1] - attempts[index];
                assert!(
                    gap >= Duration::from_millis(*delay_ms),
                    "{op:?}/{reply:?}: attempt gap {index} was {gap:?}"
                );
            }
            let announced: Vec<&str> = run
                .stderr
                .lines()
                .filter_map(|line| line.split_once("retrying in ")?.1.split_once('s'))
                .map(|(delay, _)| delay)
                .collect();
            assert_eq!(
                announced,
                [
                    "0.020", "0.040", "0.080", "0.080", "0.080", "0.080", "0.080"
                ],
                "{op:?}/{reply:?}"
            );
        });
    }

    #[test]
    fn the_shipped_schedule_doubles_to_its_cap_for_every_operation() {
        for op in OPS {
            let run = install(
                plan_for(op, vec![Reply::Status(503)]),
                version_for(op),
                Timing::ShippedWithRecordedSleep,
            );
            assert!(!run.success, "{op:?}: {}", run.output());
            assert_eq!(run.attempts(op).len(), MAX_ATTEMPTS, "{op:?}");
            let sleeps = run.sleeps.clone().unwrap_or_default();
            assert_eq!(
                sleeps.lines().collect::<Vec<_>>(),
                [
                    "2.000", "4.000", "8.000", "16.000", "30.000", "30.000", "30.000"
                ],
                "{op:?}"
            );
        }
    }

    #[test]
    fn permanent_failure_stops_after_one_attempt_and_names_what_is_missing() {
        for op in OPS {
            let run = install(
                plan_for(op, vec![Reply::Status(404)]),
                version_for(op),
                Timing::Scaled,
            );
            assert!(!run.success, "{op:?}: {}", run.output());
            run.assert_nothing_installed();
            assert_eq!(run.attempts(op).len(), 1, "{op:?}");
            let (name, url) = run.expected_name_and_url(op);
            let failure = run.stderr.lines().last().unwrap();
            assert!(
                failure.contains(&name) && failure.contains(&url) && failure.contains("not found"),
                "{op:?}: {failure}"
            );
            assert!(!run.stderr.contains("retrying"), "{op:?}: {}", run.stderr);
        }
    }

    #[test]
    fn a_forbidden_response_is_not_retried() {
        let run = install(
            plan_for(Op::Archive, vec![Reply::Status(403)]),
            TAG,
            Timing::Scaled,
        );
        assert!(!run.success, "{}", run.output());
        assert_eq!(run.attempts(Op::Archive).len(), 1);
        assert!(run.stderr.contains("failed (HTTP 403)"), "{}", run.stderr);
    }

    #[test]
    fn an_archive_with_a_mismatched_checksum_is_refused() {
        let run = install(
            plan_for(Op::Checksum, vec![Reply::WrongChecksum]),
            TAG,
            Timing::Scaled,
        );
        assert!(!run.success, "{}", run.output());
        assert!(run.stderr.contains("refusing to install"), "{}", run.stderr);
        assert_eq!(run.attempts(Op::Checksum).len(), 1);
        run.assert_nothing_installed();
    }

    #[test]
    fn a_lookup_without_a_tag_fails_with_a_clear_message() {
        let run = install(
            plan_for(Op::Lookup, vec![Reply::NoTag]),
            "latest",
            Timing::Scaled,
        );
        assert!(!run.success, "{}", run.output());
        assert!(run.stderr.contains("has no tag_name"), "{}", run.stderr);
        assert!(run.attempts(Op::Archive).is_empty());
    }

    #[test]
    fn a_lookup_with_an_unsafe_tag_is_refused_before_any_download() {
        for tag in ["../../evil", "v1/x", "-v1", "v1 x"] {
            let run = install(
                plan_for(Op::Lookup, vec![Reply::Tag(tag)]),
                "latest",
                Timing::Scaled,
            );
            assert!(!run.success, "{tag}: {}", run.output());
            assert!(
                run.stderr.contains(&format!("invalid tag '{tag}'")),
                "{tag}: {}",
                run.stderr
            );
            assert!(run.attempts(Op::Archive).is_empty(), "{tag}");
            run.assert_nothing_installed();
        }
    }

    #[test]
    fn sampled_408_429_and_5xx_statuses_are_retried() {
        let statuses = [408, 429, 500, 502].map(Reply::Status);
        for_each_case(&statuses, |op, reply| {
            let run = install(
                plan_for(op, vec![reply, Reply::Valid]),
                version_for(op),
                Timing::Scaled,
            );
            assert!(run.success, "{op:?}/{reply:?}: {}", run.output());
            assert_eq!(run.attempts(op).len(), 2, "{op:?}/{reply:?}");
        });
    }

    #[test]
    fn a_refused_connection_is_retried_until_the_attempts_run_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let run = install_with(Plan::new(), TAG, Timing::Scaled, |cmd| {
            cmd.env("SCREENCOMP_INSTALL_GITHUB_URL", &closed);
        });
        assert!(!run.success, "{}", run.output());
        run.assert_nothing_installed();
        let expected = format!("failed after {MAX_ATTEMPTS} attempts (last error: curl exit 7)");
        assert!(run.stderr.contains(&expected), "{}", run.stderr);
    }

    #[test]
    fn transport_failures_only_a_stub_can_provoke_are_retried() {
        // Proxy and DNS resolution, HTTP/2 framing and stream, TLS handshake, send.
        for code in [5, 6, 16, 35, 55, 56, 92] {
            let stub = CurlStub::exiting_with(code);
            let run = stub.install();
            assert!(!run.success, "exit {code}: {}", run.output());
            assert_eq!(stub.calls().len(), MAX_ATTEMPTS, "exit {code}");
            let expected =
                format!("failed after {MAX_ATTEMPTS} attempts (last error: curl exit {code})");
            assert!(run.stderr.contains(&expected), "{}", run.stderr);
        }
    }

    #[test]
    fn an_untrusted_certificate_is_not_retried() {
        let stub = CurlStub::exiting_with(60);
        let run = stub.install();
        assert!(!run.success, "{}", run.output());
        assert_eq!(stub.calls().len(), 1);
        assert!(
            run.stderr.contains("failed (curl exit 60)"),
            "{}",
            run.stderr
        );
        assert!(!run.stderr.contains("retrying"), "{}", run.stderr);
    }

    #[test]
    fn a_permanent_curl_failure_stops_at_once_and_names_the_exit_code() {
        let run = install_with(Plan::new(), TAG, Timing::Scaled, |cmd| {
            cmd.env("SCREENCOMP_INSTALL_GITHUB_URL", "unsupported://host");
        });
        assert!(!run.success, "{}", run.output());
        run.assert_nothing_installed();
        let failure = run.stderr.lines().last().unwrap();
        assert!(
            failure.contains("failed (curl exit 1)") && failure.contains("unsupported://host"),
            "{failure}"
        );
        assert!(!run.stderr.contains("retrying"), "{}", run.stderr);
    }

    #[test]
    fn a_lookup_without_a_token_goes_unauthenticated() {
        let run = install_with(Plan::new(), "latest", Timing::Scaled, |cmd| {
            cmd.env_remove("GH_TOKEN");
        });
        assert!(run.success, "{}", run.output());
        assert!(!run.attempts(Op::Lookup).is_empty());
        assert!(run.hits.iter().all(|hit| hit.authorization.is_none()));
    }

    #[test]
    fn a_malformed_timing_setting_stops_before_any_request() {
        for name in [
            "SCREENCOMP_INSTALL_ATTEMPT_TIMEOUT",
            "SCREENCOMP_INSTALL_RETRY_BASE_MS",
            "SCREENCOMP_INSTALL_RETRY_CAP_MS",
        ] {
            for value in ["0", "-1", "soon", "1.5", "99999999999"] {
                let run = install_with(Plan::new(), TAG, Timing::Scaled, |cmd| {
                    cmd.env(name, value);
                });
                assert!(!run.success, "{name}={value}: {}", run.output());
                assert!(
                    run.stderr.contains(name) && run.stderr.contains(&format!("got '{value}'")),
                    "{name}={value}: {}",
                    run.stderr
                );
                assert!(run.hits.is_empty(), "{name}={value}");
                run.assert_nothing_installed();
            }
        }
    }

    #[test]
    fn a_token_with_config_metacharacters_reaches_the_server_verbatim() {
        let token = r#"a"b\c"#;
        let run = install_with(Plan::new(), "latest", Timing::Scaled, |cmd| {
            cmd.env("GH_TOKEN", token);
        });
        assert!(run.success, "{}", run.output());
        let lookup = run.hits.iter().find(|hit| hit.op == Op::Lookup).unwrap();
        assert_eq!(
            lookup.authorization.as_deref().map(str::trim_end),
            Some(format!("Bearer {token}").as_str())
        );
    }

    #[test]
    fn a_token_with_a_newline_is_refused_before_any_request() {
        let run = install_with(Plan::new(), "latest", Timing::Scaled, |cmd| {
            cmd.env("GH_TOKEN", "abc\nurl = \"http://elsewhere\"");
        });
        assert!(!run.success, "{}", run.output());
        assert!(run.hits.is_empty());
        assert!(
            run.stderr.contains("without control characters"),
            "{}",
            run.stderr
        );
        run.assert_nothing_installed();
    }

    /// Asserted on curl's arguments: a connection that never completes needs an
    /// unroutable address, which an offline suite cannot rely on.
    #[test]
    fn every_attempt_bounds_its_connection_and_total_time() {
        let stub = CurlStub::exiting_with(28);
        let run = stub.install();
        assert!(!run.success, "{}", run.output());
        let calls = stub.calls();
        assert_eq!(calls.len(), MAX_ATTEMPTS);
        for call in calls {
            assert!(
                call.contains("--connect-timeout 10") && call.contains("--max-time 2"),
                "{call}"
            );
        }
    }

    /// The install of a `latest` whose lookup answers with exactly `body`.
    fn install_latest_answering(body: &'static str) -> Install {
        install(
            plan_for(Op::Lookup, vec![Reply::Raw(body)]),
            "latest",
            Timing::Scaled,
        )
    }

    fn assert_lookup_refused(body: &str, run: &Install, expected: &str) {
        assert!(!run.success, "{body}: {}", run.output());
        let failure = run.stderr.lines().last().unwrap_or_default();
        assert!(
            failure.contains(expected)
                && failure.contains("latest release of owner/repo")
                && failure.contains("/api/repos/owner/repo/releases/latest"),
            "{body}: {failure}"
        );
        assert_eq!(run.attempts(Op::Lookup).len(), 1, "{body}");
        assert!(run.attempts(Op::Archive).is_empty(), "{body}");
        run.assert_nothing_installed();
    }

    #[test]
    fn a_malformed_release_response_is_refused_before_any_download() {
        for body in [
            r#"{"tag_name":"v9.9.9""#,
            r#"{"tag_name":"v9.9.9"} trailing"#,
            r#"{"tag_name":"v9.9.9",}"#,
            r#"{"tag_name":"v9.9.9","x":01}"#,
            r#"{"tag_name":"v9.9.9","x":1.}"#,
            r#"{"tag_name":"v9.9.9","x":"\q"}"#,
            r#"{"tag_name":"v9.9.9","x":"\u12"}"#,
            "{\"tag_name\":\"v9.9.9\",\"x\":\"a\nb\"}",
            r#"{"x":truex,"tag_name":"v9.9.9"}"#,
            r#"{"a":[1,],"tag_name":"v9.9.9"}"#,
            r#"{"a":{"b"},"tag_name":"v9.9.9"}"#,
            r#"[{"tag_name":"v9.9.9"}]"#,
        ] {
            let run = install_latest_answering(body);
            assert_lookup_refused(body, &run, "is not a valid JSON object");
        }
    }

    #[test]
    fn a_tag_name_only_in_a_nested_value_is_not_the_release_tag() {
        for body in [
            r#"{"release":{"tag_name":"v9.9.9"}}"#,
            r#"{"assets":[{"tag_name":"v9.9.9"}]}"#,
            r#"{"body":"\"tag_name\":\"v9.9.9\""}"#,
        ] {
            let run = install_latest_answering(body);
            assert_lookup_refused(body, &run, "has no tag_name");
        }
    }

    #[test]
    fn a_duplicate_top_level_tag_name_is_refused() {
        for body in [
            r#"{"tag_name":"v9.9.9","tag_name":"v9.9.9"}"#,
            r#"{"tag_name":"v9.9.9","x":{},"tag\u005fname":"v0.0.1"}"#,
        ] {
            let run = install_latest_answering(body);
            assert_lookup_refused(body, &run, "has more than one tag_name");
        }
    }

    #[test]
    fn a_non_string_tag_name_is_refused() {
        for body in [
            r#"{"tag_name":9}"#,
            r#"{"tag_name":null}"#,
            r#"{"tag_name":["v9.9.9"]}"#,
        ] {
            let run = install_latest_answering(body);
            assert_lookup_refused(body, &run, "has a non-string tag_name");
        }
    }

    #[test]
    fn a_tag_that_decodes_to_something_unsafe_is_refused() {
        for (body, decoded) in [
            (r#"{"tag_name":"..\u002fevil"}"#, "../evil"),
            (r#"{"tag_name":"v9.9.9\n"}"#, "v9.9.9\n"),
        ] {
            let run = install_latest_answering(body);
            assert!(!run.success, "{body}: {}", run.output());
            assert!(
                run.stderr.contains(&format!("invalid tag '{decoded}'")),
                "{body}: {}",
                run.stderr
            );
            assert!(run.attempts(Op::Archive).is_empty(), "{body}");
            run.assert_nothing_installed();
        }
    }

    #[test]
    fn the_top_level_tag_is_read_past_nesting_escapes_and_formatting() {
        for body in [
            r#"{"tag\u005fname":"v9\u002e9\u002e9"}"#,
            r#"{"author":{"tag_name":"v0.0.1"},"assets":[{"tag_name":"v0.0.2"}],"tag_name":"v9.9.9"}"#,
            "{\n  \"id\" : -1.5e+3 ,\r\n\t\"ok\" : true , \"no\" : false , \"nil\" : null ,\n  \"empty\" : { } , \"list\" : [ ] , \"zero\" : 0 ,\n  \"body\" : \"h\u{e9}llo \\\" \\\\ \\/ \\b \\f \\n \\r \\t \\u00E9\" ,\n  \"tag_name\" : \"v9.9.9\"\n}\n",
        ] {
            let run = install_latest_answering(body);
            assert!(run.success, "{body}: {}", run.output());
            let archive = run.hits.iter().filter(|hit| hit.op == Op::Archive).count();
            assert_eq!(archive, 1, "{body}");
            assert!(
                run.stdout
                    .contains(&format!("Installing screencomp {TAG} ")),
                "{body}"
            );
        }
    }

    #[test]
    fn nesting_is_accepted_up_to_its_limit_and_refused_beyond_it() {
        let nested = |arrays: usize| -> &'static str {
            let body = format!(
                r#"{{"x":{}{},"tag_name":"{TAG}"}}"#,
                "[".repeat(arrays),
                "]".repeat(arrays)
            );
            Box::leak(body.into_boxed_str())
        };
        let within = install_latest_answering(nested(63));
        assert!(within.success, "{}", within.output());
        let body = nested(64);
        let beyond = install_latest_answering(body);
        assert_lookup_refused(body, &beyond, "is not a valid JSON object");
    }

    #[test]
    fn the_shipped_attempt_timeout_is_sixty_seconds() {
        let stub = CurlStub::exiting_with(28);
        let run = stub.install_adjusted(TAG, |cmd| {
            cmd.env_remove("SCREENCOMP_INSTALL_ATTEMPT_TIMEOUT");
        });
        assert!(!run.success, "{}", run.output());
        let calls = stub.calls();
        assert_eq!(calls.len(), MAX_ATTEMPTS);
        assert!(
            calls.iter().all(|call| call.contains("--max-time 60")),
            "{calls:?}"
        );
    }

    #[test]
    fn the_token_stays_out_of_curls_arguments() {
        let stub = CurlStub::exiting_with(28);
        let run = stub.install_adjusted("latest", |_| {});
        assert!(!run.success, "{}", run.output());
        let calls = stub.calls();
        assert_eq!(calls.len(), MAX_ATTEMPTS);
        assert!(
            calls.iter().all(|call| !call.contains("test-token")),
            "{calls:?}"
        );
    }

    #[test]
    fn an_awk_that_cannot_run_is_reported_with_its_exit_code() {
        let dir = TempDir::new().unwrap();
        let awk = dir.path().join("awk");
        std::fs::write(&awk, "#!/bin/sh\nexit 2\n").unwrap();
        let made_executable = command("chmod").arg("+x").arg(&awk).status().unwrap();
        assert!(made_executable.success());
        let path = format!(
            "{}:{}",
            path_str(dir.path()),
            std::env::var("PATH").unwrap()
        );
        let run = install_with(Plan::new(), "latest", Timing::Scaled, |cmd| {
            cmd.env("PATH", path);
        });
        assert_lookup_refused("awk stand-in", &run, "could not be parsed (awk exit 2)");
    }

    /// the repository's download scripts, invoked from the repository
    /// root exactly as the workflows and recipes invoke them.
    mod repo_tooling {
        use super::*;

        const TOOL_PATH: &str = "/tool.tar.gz";

        struct Run {
            _work: TempDir,
            dest: PathBuf,
            hits: Vec<Hit>,
            url: String,
            success: bool,
            stderr: String,
            elapsed: Duration,
            sleeps: Vec<String>,
        }

        /// A PATH whose `sleep` records its argument instead of waiting.
        fn recording_sleep(work: &Path) -> (String, PathBuf) {
            let bin = work.join("bin");
            std::fs::create_dir(&bin).unwrap();
            let log = work.join("sleeps");
            let stub = bin.join("sleep");
            std::fs::write(
                &stub,
                format!("#!/bin/sh\necho \"$1\" >>'{}'\n", path_str(&log)),
            )
            .unwrap();
            assert!(
                command("chmod")
                    .arg("+x")
                    .arg(&stub)
                    .status()
                    .unwrap()
                    .success()
            );
            let path = std::env::var("PATH").unwrap();
            (format!("{}:{path}", path_str(&bin)), log)
        }

        fn download(replies: Vec<Reply>, timing: Timing) -> Run {
            archive();
            let server = start_server(plan_for(Op::Archive, replies));
            let work = TempDir::new().unwrap();
            let dest = work.path().join("tool.tar.gz");
            let url = format!("{}{TOOL_PATH}", server.url);
            let mut cmd = command("bash");
            cmd.arg("scripts/download.sh")
                .arg(&url)
                .arg(&dest)
                .current_dir(env!("CARGO_MANIFEST_DIR"));
            let mut sleep_log = None;
            match timing {
                Timing::Scaled => {
                    cmd.env("SCREENCOMP_DOWNLOAD_ATTEMPT_TIMEOUT", "2")
                        .env("SCREENCOMP_DOWNLOAD_RETRY_BASE_MS", "20")
                        .env("SCREENCOMP_DOWNLOAD_RETRY_CAP_MS", "80");
                }
                Timing::ShippedWithRecordedSleep => {
                    let (path, log) = recording_sleep(work.path());
                    cmd.env("PATH", path);
                    sleep_log = Some(log);
                }
            }
            let stdout_path = work.path().join("stdout");
            let stderr_path = work.path().join("stderr");
            let (status, elapsed) = run_to_deadline(cmd, &stdout_path, &stderr_path);
            let hits = server.hits.lock().unwrap().clone();
            Run {
                dest,
                hits,
                url,
                success: status.success(),
                stderr: std::fs::read_to_string(&stderr_path).unwrap(),
                elapsed,
                sleeps: sleep_log
                    .and_then(|log| std::fs::read_to_string(log).ok())
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect(),
                _work: work,
            }
        }

        fn assert_no_files_left(run: &Run) {
            let dir = run.dest.parent().unwrap();
            let leftovers: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("tool.tar.gz"))
                .collect();
            assert!(leftovers.is_empty(), "{leftovers:?}: {}", run.stderr);
        }

        fn for_each_transient(check: impl Fn(Reply) + Sync) {
            std::thread::scope(|scope| {
                for reply in TRANSIENT {
                    let check = &check;
                    scope.spawn(move || check(reply));
                }
            });
        }

        #[test]
        fn a_transient_failure_recovers_into_a_file_that_extracts() {
            for_each_transient(|reply| {
                let run = download(vec![reply, reply, Reply::Valid], Timing::Scaled);
                assert!(run.success, "{reply:?}: {}", run.stderr);
                assert_eq!(run.hits.len(), 3, "{reply:?}");
                let out = run.dest.parent().unwrap().join("out");
                std::fs::create_dir(&out).unwrap();
                let extracted = command("tar")
                    .arg("-xzf")
                    .arg(&run.dest)
                    .arg("-C")
                    .arg(&out)
                    .status()
                    .unwrap();
                assert!(extracted.success(), "{reply:?}");
                let version = command(out.join("screencomp"))
                    .arg("--version")
                    .output()
                    .unwrap();
                assert!(version.status.success(), "{reply:?}");
            });
        }

        #[test]
        fn a_persistent_transient_failure_is_bounded_names_the_url_and_leaves_no_file() {
            for_each_transient(|reply| {
                let run = download(vec![reply], Timing::Scaled);
                assert!(!run.success, "{reply:?}: {}", run.stderr);
                assert_eq!(run.hits.len(), MAX_ATTEMPTS, "{reply:?}");
                assert_no_files_left(&run);
                let backoff: u64 = SCALED_DELAYS_MS.iter().sum();
                let limit =
                    Duration::from_millis(backoff) + Duration::from_secs(MAX_ATTEMPTS as u64 + 30);
                assert!(run.elapsed < limit, "{reply:?}: {:?}", run.elapsed);
                let failure = run.stderr.lines().last().unwrap();
                assert!(
                    failure.contains(&format!("failed after {MAX_ATTEMPTS} attempts"))
                        && failure.contains(&run.url),
                    "{reply:?}: {failure}"
                );
                for (index, delay_ms) in SCALED_DELAYS_MS.iter().enumerate() {
                    let gap = run.hits[index + 1].at - run.hits[index].at;
                    assert!(
                        gap >= Duration::from_millis(*delay_ms),
                        "{reply:?}: attempt gap {index} was {gap:?}"
                    );
                }
            });
        }

        #[test]
        fn the_shipped_schedule_doubles_to_its_cap() {
            let run = download(vec![Reply::Status(503)], Timing::ShippedWithRecordedSleep);
            assert!(!run.success, "{}", run.stderr);
            assert_eq!(
                run.sleeps,
                [
                    "2.000", "4.000", "8.000", "16.000", "30.000", "30.000", "30.000"
                ]
            );
        }

        #[test]
        fn a_missing_download_stops_after_one_attempt() {
            let run = download(vec![Reply::Status(404)], Timing::Scaled);
            assert!(!run.success, "{}", run.stderr);
            assert_eq!(run.hits.len(), 1);
            assert_no_files_left(&run);
            assert_eq!(
                run.stderr.lines().last().unwrap(),
                format!("download not found (HTTP 404): {}", run.url)
            );
        }

        /// A `sleep` on the PATH that logs backoffs (fractional seconds, or all
        /// sleeps when `record_all`) without waiting, and really waits otherwise.
        fn sleep_stub(work: &Path, record_all: bool) -> (String, PathBuf) {
            let bin = work.join("sleep-bin");
            std::fs::create_dir(&bin).unwrap();
            let log = work.join("sleeps");
            let stub = bin.join("sleep");
            let whole = if record_all {
                ""
            } else {
                "exec /bin/sleep \"$1\""
            };
            std::fs::write(
                &stub,
                format!(
                    "#!/bin/sh\ncase \"$1\" in\n*.*) ;;\n*) {whole} ;;\nesac\necho \"$1\" >>'{}'\n",
                    path_str(&log)
                ),
            )
            .unwrap();
            make_executable(&stub);
            let path = std::env::var("PATH").unwrap();
            (format!("{}:{path}", path_str(&bin)), log)
        }

        fn make_executable(path: &Path) {
            let status = command("chmod").arg("+x").arg(path).status().unwrap();
            assert!(status.success());
        }

        fn read_lines(path: &Path) -> Vec<String> {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        /// Answers every request with `reply` until `healthy_after` backoffs are
        /// logged, then serves the archive: the failing phase spans whole
        /// cargo-binstall attempts however many requests each one makes.
        fn start_binstall_server(
            reply: Reply,
            sleeps: PathBuf,
            healthy_after: Option<usize>,
        ) -> String {
            archive();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/screencomp.tgz", listener.local_addr().unwrap());
            std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let sleeps = sleeps.clone();
                    std::thread::spawn(move || {
                        read_request(&mut stream);
                        if healthy_after.is_some_and(|after| read_lines(&sleeps).len() >= after) {
                            respond(&mut stream, 200, archive());
                            return;
                        }
                        match reply {
                            Reply::Status(status) => respond(&mut stream, status, b"failure"),
                            Reply::Hang => {
                                let mut sink = [0_u8; 64];
                                while matches!(stream.read(&mut sink), Ok(read) if read > 0) {}
                            }
                            _ => drop(stream),
                        }
                    });
                }
            });
            url
        }

        /// How one real cargo-binstall failure is provoked.
        #[derive(Clone, Copy, Debug)]
        struct BinstallCase {
            reply: Reply,
            attempt_timeout: &'static str,
            resolution_timeout: &'static str,
        }

        const fn binstall_reply(reply: Reply) -> BinstallCase {
            BinstallCase {
                reply,
                attempt_timeout: "60",
                resolution_timeout: "120",
            }
        }

        /// The script's own per-attempt limit stops a hung cargo-binstall.
        const WATCHDOG: BinstallCase = BinstallCase {
            reply: Reply::Hang,
            attempt_timeout: "2",
            resolution_timeout: "120",
        };

        const BINSTALL_TRANSIENT: [BinstallCase; 6] = [
            binstall_reply(Reply::Status(504)),
            binstall_reply(Reply::Status(503)),
            binstall_reply(Reply::Status(429)),
            binstall_reply(Reply::Drop),
            // cargo-binstall's own resolution timeout.
            BinstallCase {
                reply: Reply::Hang,
                attempt_timeout: "60",
                resolution_timeout: "1",
            },
            WATCHDOG,
        ];

        struct BinstallRun {
            work: TempDir,
            code: Option<i32>,
            stderr: String,
            stdout: String,
            sleeps: Vec<String>,
            elapsed: Duration,
        }

        /// Runs `scripts/binstall.sh` over the real cargo-binstall, as the recipes
        /// do, resolving a crate whose only package URL is the local server.
        fn binstall(case: BinstallCase, healthy_after: Option<usize>) -> BinstallRun {
            let present = command("cargo-binstall").arg("-V").output();
            assert!(
                present.is_ok_and(|output| output.status.success()),
                "cargo-binstall is not on PATH; ACTION: run 'just bootstrap'"
            );
            let work = TempDir::new().unwrap();
            let manifest = work.path().join("crate");
            std::fs::create_dir_all(manifest.join("src")).unwrap();
            std::fs::write(manifest.join("src/main.rs"), "fn main() {}\n").unwrap();
            std::fs::write(
                manifest.join("Cargo.toml"),
                "[package]\nname = \"screencomp\"\nversion = \"9.9.9\"\nedition = \"2021\"\n",
            )
            .unwrap();
            let (path, sleep_log) = sleep_stub(work.path(), false);
            let url = start_binstall_server(case.reply, sleep_log.clone(), healthy_after);
            let mut cmd = command("bash");
            cmd.arg("scripts/binstall.sh")
                .args(["--no-confirm", "--allow-insecure-http", "--no-track"])
                .args(["--strategies", "crate-meta-data"])
                .args(["--disable-strategies", "compile,quick-install"])
                .args(["--no-discover-github-token", "--disable-telemetry"])
                .args(["--maximum-resolution-timeout", case.resolution_timeout])
                .arg("--manifest-path")
                .arg(&manifest)
                .args(["--pkg-url", &url, "--pkg-fmt", "tgz"])
                .args(["--bin-dir", "{ bin }{ binary-ext }"])
                .arg("--root")
                .arg(work.path().join("root"))
                .arg("screencomp")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .env_remove("GITHUB_TOKEN")
                .env_remove("GH_TOKEN")
                .env("CARGO_HOME", work.path().join("cargo-home"))
                .env("PATH", path)
                .env("SCREENCOMP_BINSTALL_ATTEMPT_TIMEOUT", case.attempt_timeout);
            let stdout_path = work.path().join("stdout");
            let stderr_path = work.path().join("stderr");
            let (status, elapsed) = run_to_deadline(cmd, &stdout_path, &stderr_path);
            BinstallRun {
                code: status.code(),
                stderr: std::fs::read_to_string(&stderr_path).unwrap(),
                stdout: std::fs::read_to_string(&stdout_path).unwrap(),
                sleeps: read_lines(&sleep_log),
                elapsed,
                work,
            }
        }

        impl BinstallRun {
            fn output(&self) -> String {
                format!("stdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
            }

            fn binary(&self) -> PathBuf {
                self.work.path().join("root/bin/screencomp")
            }
        }

        fn for_each_binstall_case(cases: &[BinstallCase], check: impl Fn(BinstallCase) + Sync) {
            std::thread::scope(|scope| {
                for &case in cases {
                    let check = &check;
                    scope.spawn(move || check(case));
                }
            });
        }

        #[test]
        fn real_binstall_recovers_from_each_transient_failure() {
            for_each_binstall_case(&BINSTALL_TRANSIENT, |case| {
                let run = binstall(case, Some(2));
                assert_eq!(run.code, Some(0), "{case:?}: {}", run.output());
                assert_eq!(run.sleeps, ["5.000", "10.000"], "{case:?}");
                let version = command(run.binary()).arg("--version").output().unwrap();
                assert!(version.status.success(), "{case:?}");
            });
        }

        #[test]
        fn real_binstall_exhausts_bounded_attempts_on_a_persistent_transient_failure() {
            for_each_binstall_case(&BINSTALL_TRANSIENT, |case| {
                let run = binstall(case, None);
                assert_eq!(run.sleeps, ["5.000", "10.000", "20.000"], "{case:?}");
                assert!(!run.binary().exists(), "{case:?}");
                let failure = run.stderr.lines().last().unwrap();
                assert!(
                    failure.starts_with("cargo-binstall failed after 4 attempts"),
                    "{case:?}: {failure}"
                );
                assert!(run.elapsed < INSTALL_DEADLINE, "{case:?}");
            });
        }

        #[test]
        fn a_hung_binstall_is_stopped_at_each_attempts_limit() {
            let run = binstall(WATCHDOG, None);
            assert_eq!(run.code, Some(124), "{}", run.output());
            assert!(
                run.stderr.contains("last error: timed out after 2s"),
                "{}",
                run.output()
            );
            // Four 2s attempts, each stopped by TERM well inside the KILL grace.
            assert!(run.elapsed >= Duration::from_secs(8), "{:?}", run.elapsed);
            assert!(run.elapsed < Duration::from_secs(20), "{:?}", run.elapsed);
        }

        #[test]
        fn real_binstall_stops_after_one_attempt_on_a_permanent_failure() {
            for status in [404, 403] {
                let run = binstall(binstall_reply(Reply::Status(status)), None);
                assert_eq!(run.code, Some(94), "{status}: {}", run.output());
                assert!(run.sleeps.is_empty(), "{status}: {:?}", run.sleeps);
                assert_eq!(
                    run.stderr.lines().last().unwrap(),
                    "cargo-binstall failed with a permanent error (exit 94), see its output above",
                    "{status}"
                );
            }
        }

        struct SyncInstall {
            _work: TempDir,
            hits: Vec<Hit>,
            success: bool,
            output: String,
            binary: PathBuf,
            github_path: PathBuf,
            sleeps: Vec<String>,
            gh_calls: usize,
            url: String,
        }

        impl SyncInstall {
            fn attempts(&self, op: Op) -> Vec<Instant> {
                self.hits
                    .iter()
                    .filter(|hit| hit.op == op)
                    .map(|hit| hit.at)
                    .collect()
            }

            fn assert_installed(&self, context: &str) {
                let version = command(&self.binary).arg("--version").output().unwrap();
                assert!(version.status.success(), "{context}: {}", self.output);
                assert!(self.github_path.exists(), "{context}: {}", self.output);
            }

            fn assert_skipped_install(&self, context: &str) {
                // The step stays green so the demo refresh still runs.
                assert!(self.success, "{context}: {}", self.output);
                assert!(!self.binary.exists(), "{context}: {}", self.output);
                assert!(!self.github_path.exists(), "{context}: {}", self.output);
                assert!(self.output.contains("::warning::"), "{context}");
            }
        }

        /// A `gh` that reports the release's assets uploaded from poll `ready_at`
        /// (1-based) onward, and fails like an unreachable API before poll 2.
        fn gh_stub(work: &Path, ready_at: usize) -> (PathBuf, PathBuf) {
            let bin = work.join("gh-bin");
            std::fs::create_dir(&bin).unwrap();
            let log = work.join("gh-calls");
            let stub = bin.join("gh");
            std::fs::write(
                &stub,
                format!(
                    "#!/bin/sh\necho \"$*\" >>'{log}'\ncalls=$(wc -l <'{log}')\n\
                     [ \"$calls\" -gt 1 ] || exit 1\n\
                     [ \"$calls\" -ge {ready_at} ] || {{ echo uploading.tmp; exit 0; }}\n\
                     stem=screencomp-{TAG}-x86_64-unknown-linux-gnu\n\
                     echo \"$stem.tar.gz\"\necho \"$stem.sha256\"\n",
                    log = path_str(&log)
                ),
            )
            .unwrap();
            make_executable(&stub);
            (bin, log)
        }

        /// Runs sync-demo.yml's install step script against the local server.
        fn sync_install(plan: Plan, release_ready_at: Option<usize>) -> SyncInstall {
            archive();
            let server = start_server(plan);
            let work = TempDir::new().unwrap();
            let home = work.path().join("home");
            std::fs::create_dir(&home).unwrap();
            let github_path = work.path().join("github-path");
            let mut cmd = command("bash");
            cmd.arg("scripts/sync-demo-install.sh")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .env("HAVE_TOKEN", "present")
                .env("GITHUB_TOKEN", "test-token")
                .env("GITHUB_REPOSITORY", "owner/repo")
                .env("GITHUB_PATH", &github_path)
                .env("HOME", &home)
                .env("SCREENCOMP_INSTALL_GITHUB_URL", &server.url)
                .env(
                    "SCREENCOMP_INSTALL_GITHUB_API_URL",
                    format!("{}/api", server.url),
                )
                .env("SCREENCOMP_DOWNLOAD_ATTEMPT_TIMEOUT", "2")
                .env("SCREENCOMP_DOWNLOAD_RETRY_BASE_MS", "20")
                .env("SCREENCOMP_DOWNLOAD_RETRY_CAP_MS", "80");
            let mut sleep_log = None;
            let mut gh_log = None;
            match release_ready_at {
                Some(ready_at) => {
                    let (path, sleeps) = sleep_stub(work.path(), true);
                    let (gh, calls) = gh_stub(work.path(), ready_at);
                    cmd.env("EVENT_NAME", "release")
                        .env("RELEASE_TAG", TAG)
                        .env("PATH", format!("{}:{path}", path_str(&gh)));
                    sleep_log = Some(sleeps);
                    gh_log = Some(calls);
                }
                None => {
                    cmd.env("EVENT_NAME", "push").env("RELEASE_TAG", "");
                }
            }
            let stdout_path = work.path().join("stdout");
            let stderr_path = work.path().join("stderr");
            let (status, _) = run_to_deadline(cmd, &stdout_path, &stderr_path);
            SyncInstall {
                hits: server.hits.lock().unwrap().clone(),
                success: status.success(),
                output: format!(
                    "{}{}",
                    std::fs::read_to_string(&stdout_path).unwrap(),
                    std::fs::read_to_string(&stderr_path).unwrap()
                ),
                binary: home.join(".local/bin/screencomp"),
                github_path,
                sleeps: sleep_log.map_or_else(Vec::new, |log| read_lines(&log)),
                gh_calls: gh_log.map_or(0, |log| read_lines(&log).len()),
                url: server.url,
                _work: work,
            }
        }

        #[test]
        fn the_sync_demo_install_recovers_from_transient_failures_on_every_request() {
            for_each_case(&TRANSIENT, |op, reply| {
                let run = sync_install(plan_for(op, vec![reply, reply, Reply::Valid]), None);
                run.assert_installed(&format!("{op:?}/{reply:?}"));
                assert_eq!(
                    run.attempts(op).len(),
                    3,
                    "{op:?}/{reply:?}: {}",
                    run.output
                );
            });
        }

        #[test]
        fn the_sync_demo_install_gives_up_after_bounded_growing_attempts() {
            for_each_case(&TRANSIENT, |op, reply| {
                let context = format!("{op:?}/{reply:?}");
                let run = sync_install(plan_for(op, vec![reply]), None);
                run.assert_skipped_install(&context);
                let attempts = run.attempts(op);
                assert_eq!(attempts.len(), MAX_ATTEMPTS, "{context}: {}", run.output);
                assert!(
                    run.output
                        .contains(&format!("failed after {MAX_ATTEMPTS} attempts"))
                        && run.output.contains(&run.url),
                    "{context}: {}",
                    run.output
                );
                for (index, delay_ms) in SCALED_DELAYS_MS.iter().enumerate() {
                    let gap = attempts[index + 1] - attempts[index];
                    assert!(
                        gap >= Duration::from_millis(*delay_ms),
                        "{context}: attempt gap {index} was {gap:?}"
                    );
                }
            });
        }

        #[test]
        fn the_sync_demo_install_stops_at_once_on_a_404() {
            for op in OPS {
                let run = sync_install(plan_for(op, vec![Reply::Status(404)]), None);
                run.assert_skipped_install(&format!("{op:?}"));
                assert_eq!(run.attempts(op).len(), 1, "{op:?}: {}", run.output);
                assert!(
                    run.output.contains("download not found (HTTP 404)"),
                    "{op:?}: {}",
                    run.output
                );
            }
        }

        #[test]
        fn the_sync_demo_install_sends_its_token_only_to_the_api() {
            let run = sync_install(Plan::new(), None);
            run.assert_installed("happy path");
            for hit in &run.hits {
                let expected = (hit.op == Op::Lookup).then(|| "Bearer test-token".to_owned());
                assert_eq!(hit.authorization, expected, "{:?}", hit.op);
            }
        }

        #[test]
        fn a_release_waits_for_its_assets_before_a_single_install() {
            let run = sync_install(Plan::new(), Some(4));
            run.assert_installed("release");
            assert_eq!(run.gh_calls, 4, "{}", run.output);
            assert_eq!(run.sleeps, ["15", "30", "60"], "{}", run.output);
            for op in OPS {
                assert_eq!(run.attempts(op).len(), 1, "{op:?}: {}", run.output);
            }
        }

        #[test]
        fn a_release_whose_assets_never_arrive_skips_the_install_without_requesting_them() {
            let run = sync_install(Plan::new(), Some(usize::MAX >> 1));
            run.assert_skipped_install("release");
            assert_eq!(run.gh_calls, 8, "{}", run.output);
            assert_eq!(
                run.sleeps,
                ["15", "30", "60", "60", "60", "60", "60"],
                "{}",
                run.output
            );
            assert!(run.hits.is_empty(), "{}", run.output);
        }

        #[test]
        fn install_sh_still_runs_standalone_when_piped_to_sh() {
            archive();
            let server = start_server(Plan::new());
            let work = TempDir::new().unwrap();
            let dir = work.path().join("bin");
            let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh");
            let mut cmd = command("sh");
            cmd.args(["-s", "--", "--to"])
                .arg(&dir)
                .current_dir(work.path())
                .env_remove("SCREENCOMP_INSTALL_DOWNLOADER")
                .env("SCREENCOMP_INSTALL_GITHUB_URL", &server.url)
                .env(
                    "SCREENCOMP_INSTALL_GITHUB_API_URL",
                    format!("{}/api", server.url),
                )
                .stdin(std::fs::File::open(script).unwrap());
            let stdout_path = work.path().join("stdout");
            let stderr_path = work.path().join("stderr");
            let (status, _) = run_to_deadline(cmd, &stdout_path, &stderr_path);
            let stderr = std::fs::read_to_string(&stderr_path).unwrap();
            assert!(status.success(), "{stderr}");
            let version = command(dir.join("screencomp"))
                .arg("--version")
                .output()
                .unwrap();
            assert!(version.status.success(), "{stderr}");
        }

        /// Every file whose shell performs this repository's own downloads.
        fn download_surfaces() -> Vec<(String, String)> {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let mut surfaces: Vec<PathBuf> = std::fs::read_dir(root.join(".github/workflows"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            surfaces.push(root.join("justfile"));
            surfaces.push(root.join("scripts/setup.sh"));
            surfaces
                .into_iter()
                .map(|path| {
                    let text = std::fs::read_to_string(&path).unwrap();
                    (path_str(&path), text)
                })
                .collect()
        }

        #[test]
        fn workflows_and_recipes_only_download_through_the_retrying_mechanism() {
            let mut routed = 0;
            for (path, text) in download_surfaces() {
                for line in text.lines().map(str::trim) {
                    // `have curl` is setup.sh's prerequisite check, not a fetch.
                    if line.starts_with('#') || line.starts_with("have curl") {
                        continue;
                    }
                    assert!(
                        !line
                            .split_whitespace()
                            .any(|word| word == "curl" || word == "wget"),
                        "{path}: `{line}` bypasses scripts/download.sh"
                    );
                    assert!(
                        !line.contains("binstall --no-confirm")
                            || line.contains("bash scripts/binstall.sh --no-confirm"),
                        "{path}: `{line}` is not retried"
                    );
                    routed += usize::from(line.contains("bash scripts/download.sh"));
                }
            }
            // ci.yml 2, release-plz.yml 1, justfile 4, setup.sh 1.
            assert_eq!(routed, 8);
            assert!(
                repo_file(".github/workflows/sync-demo.yml")
                    .contains("run: bash scripts/sync-demo-install.sh")
            );
        }
    }
}
