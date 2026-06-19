//! `screencomp doctor` — preflight a capture (or the repo's setup) before relying
//! on it.
//!
//! The two surprises a first-time integrator hits are an unexpected arch (so
//! `--arch auto` scopes to an empty subtree) and a capture written to the wrong
//! path (so the tree is silently ignored) — both of which surface only as a
//! confusing *empty diff* downstream. This command makes them explicit up front:
//! it prints the resolved arch and the exact path it inspected, lists the
//! projects and shot counts it found, and flags layout problems (no shots, or
//! `.png` files stranded at the root). With `--exit-code` it doubles as a CI gate.
//!
//! `--env` checks the *other* class of surprise — a setup that looks wired but
//! isn't: a scaffolded pre-push guard that was never enabled (`core.hooksPath`
//! unset), an installed CLI that has drifted from the version the workflow pins,
//! and a missing Docker that will fail capture. These otherwise bite only at push
//! or CI time; `--env` surfaces them in one command.

use std::io::Write;

use camino::Utf8Path;
use serde::Serialize;

use super::{Ctx, arch, discover_scoped, resolve_arch, scan_scoped, write_err};
use crate::cli::{DoctorArgs, OutputFormat};
use crate::domain::classify::classify;
use crate::domain::layout::LayoutScan;
use crate::domain::preflight;
use crate::errors::AppError;
use crate::io::{fs, host};

pub(crate) fn run(args: &DoctorArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // `--env` preflights the repository's setup (guard wired? workflow pin in
    // step with this CLI? Docker present?) rather than a capture, so it ignores
    // `--input` entirely.
    if args.env {
        return run_env(args, ctx, out);
    }
    // clap requires `--input` whenever `--env` is absent, so it is always present here.
    let input = args
        .input
        .as_deref()
        .expect("clap requires --input unless --env");

    // Resolve the arch once (an explicit `--arch`, else the host default when
    // `[capture].arches` is configured) so the report shows what was scoped and
    // which subtree was actually scanned.
    let resolved = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let plat = resolved.as_deref();
    // Whether the arch was auto-detected from the host (explicit `auto`, or the
    // config-default when no `--arch` was passed) versus named explicitly.
    let auto =
        resolved.is_some() && (args.arch.is_none() || args.arch.as_deref() == Some(arch::AUTO));
    let scoped = arch::scope(input, plat);
    // A missing scoped directory is the same hard error every command raises, so
    // a wrong `--arch` fails identically here (with a layout hint).
    let scan = scan_scoped(input, plat)?;

    // Optional sanity check against a committed manifest: catch the "everything
    // changed" arch mismatch before it reaches classify.
    let warnings = match args.baseline_manifest.as_deref() {
        Some(manifest) => baseline_warnings(manifest, input, plat)?,
        None => Vec::new(),
    };

    match args.format {
        OutputFormat::Json => {
            write_json(input, plat, &scoped, &scan, &warnings, out)?;
        }
        OutputFormat::Human if !ctx.quiet => {
            write_human(plat, auto, &scoped, &scan, &warnings, out)?;
        }
        OutputFormat::Human => {}
    }

    if args.exit_code && scan.has_problems() {
        return Ok(3);
    }
    Ok(0)
}

/// Compare the scoped capture against a committed digest `manifest` and return
/// advisory warnings for the two ways a baseline silently lies.
///
/// A screenshot's bytes depend on the CPU arch and fonts that rendered it, so a
/// baseline captured on another arch makes *every* shot look changed — the
/// single most confusing first-run failure. This flags it two ways: when the
/// manifest's `<arch>.sha256` filename names an arch other than the capture's,
/// and when the comparison finds shared shots but zero unchanged. Both are
/// advisory (they never fail the gate), since a legitimately total rewrite looks
/// the same.
fn baseline_warnings(
    manifest: &Utf8Path,
    input: &Utf8Path,
    plat: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let baseline = fs::read_manifest(manifest)?;
    let current = discover_scoped(input, plat)?;
    let host = arch::host_arch();
    // The arch the capture is meant to represent: the resolved arch when scoped,
    // else the host running this binary.
    let reference = plat.unwrap_or(&host);

    let mut warnings = Vec::new();

    // The manifest filename encodes its arch by convention (x86_64.sha256).
    if let Some(stem) = manifest.file_stem()
        && looks_like_arch_key(stem)
        && arch::canonical(stem) != reference
    {
        warnings.push(format!(
            "baseline manifest '{stem}' names a different arch than the capture ({reference}); \
             digests differ per arch/fonts, so compare like-for-like"
        ));
    }

    // Shared shot names but nothing identical: the byte-for-byte mismatch a
    // wrong-arch baseline produces.
    let c = classify(&baseline, &current).counts;
    if c.unchanged == 0 && c.changed > 0 {
        warnings.push(format!(
            "every shared shot differs from the baseline ({} changed, 0 unchanged) — usually an \
             arch/environment mismatch, not a real change; confirm the baseline was captured \
             on {host}",
            c.changed
        ));
    }

    Ok(warnings)
}

/// Whether `stem` looks like a CPU-arch key, so a manifest named after something
/// else (e.g. `baseline.sha256`) is not mistaken for one.
fn looks_like_arch_key(stem: &str) -> bool {
    matches!(stem, "x86_64" | "arm64" | "aarch64")
}

/// Stable single-line JSON contract for automation.
fn write_json(
    input: &Utf8Path,
    arch: Option<&str>,
    scoped: &Utf8Path,
    scan: &LayoutScan,
    warnings: &[String],
    out: &mut dyn Write,
) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Project<'a> {
        name: &'a str,
        shots: usize,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        input: &'a str,
        arch: Option<&'a str>,
        inspected: &'a str,
        projects: Vec<Project<'a>>,
        loose_pngs: &'a [String],
        total_shots: usize,
        warnings: &'a [String],
        ok: bool,
    }

    let report = Report {
        input: input.as_str(),
        arch,
        inspected: scoped.as_str(),
        projects: scan
            .projects
            .iter()
            .map(|p| Project {
                name: &p.name,
                shots: p.shots,
            })
            .collect(),
        loose_pngs: &scan.loose_pngs,
        total_shots: scan.total_shots(),
        warnings,
        ok: !scan.has_problems(),
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// Human-readable preflight report: resolved arch, scanned path, projects, and
/// any layout problems, ending in a single verdict line.
fn write_human(
    arch: Option<&str>,
    auto: bool,
    scoped: &Utf8Path,
    scan: &LayoutScan,
    warnings: &[String],
    out: &mut dyn Write,
) -> Result<(), AppError> {
    match arch {
        // Arch auto-detected from the host (explicit `auto` or the config default).
        Some(key) if auto => writeln!(out, "arch: {key} (auto)"),
        Some(key) => writeln!(out, "arch: {key}"),
        // No arch layer: the root is treated as project-level.
        None => writeln!(out, "arch: none (root is project-level)"),
    }
    .map_err(write_err)?;
    writeln!(out, "inspected: {scoped}").map_err(write_err)?;

    writeln!(out, "projects: {}", scan.projects.len()).map_err(write_err)?;
    for project in &scan.projects {
        writeln!(
            out,
            "  {} ({} {})",
            project.name,
            project.shots,
            if project.shots == 1 { "shot" } else { "shots" }
        )
        .map_err(write_err)?;
    }
    writeln!(out, "shots: {}", scan.total_shots()).map_err(write_err)?;

    if !scan.loose_pngs.is_empty() {
        writeln!(
            out,
            "warning: {} .png file(s) directly under the root (expected <project>/<name>.png): {}",
            scan.loose_pngs.len(),
            scan.loose_pngs.join(", ")
        )
        .map_err(write_err)?;
    }
    if scan.total_shots() == 0 {
        writeln!(out, "warning: no screenshots found under {scoped}").map_err(write_err)?;
    }
    for warning in warnings {
        writeln!(out, "warning: {warning}").map_err(write_err)?;
    }

    if scan.has_problems() {
        writeln!(out, "problems found: layout will not classify as expected")
    } else {
        writeln!(out, "ok: layout matches <project>/<name>.png")
    }
    .map_err(write_err)
}

// ----- environment preflight (`doctor --env`) -------------------------------

/// Whether the local pre-push guard is actually wired to run.
enum GuardState {
    /// Committed `.githooks/pre-push` present and `core.hooksPath` points at it.
    Enabled,
    /// The committed hook exists but Git is not pointed at it — the inert-guard
    /// gap, where the repo looks protected but nothing runs. A problem.
    PresentNotEnabled { hooks_path: Option<String> },
    /// `core.hooksPath` is set elsewhere (a different hook manager) and there is
    /// no committed `.githooks/pre-push`. Advisory: a deliberate custom setup.
    Custom { hooks_path: String },
    /// Neither a committed hook nor a `core.hooksPath`: nothing scaffolded yet.
    NotScaffolded,
}

/// The scaffolded workflow's reusable-workflow version pin relative to this CLI.
enum PinState {
    /// Pin matches the running CLI.
    Matches(String),
    /// Pin differs from the running CLI — manifest/classify behavior could drift
    /// between the local guard and CI. A problem.
    Skew { pinned: String },
    /// Workflow present but no recognizable `@v<version>` pin.
    NoPin,
    /// No scaffolded workflow at all.
    NoWorkflow,
}

/// Run the environment preflight: guard wiring, version pin, Docker presence.
fn run_env(args: &DoctorArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let dir = &args.dir;

    let committed_hook = fs::file_exists(&dir.join(".githooks/pre-push"));
    let hooks_path = host::hooks_path(dir);
    let points_at_guard = hooks_path
        .as_deref()
        .is_some_and(|p| points_at_githooks(dir, p));
    let guard = match (committed_hook, points_at_guard, hooks_path.clone()) {
        (true, true, _) => GuardState::Enabled,
        (true, false, hooks_path) => GuardState::PresentNotEnabled { hooks_path },
        (false, _, Some(hooks_path)) => GuardState::Custom { hooks_path },
        (false, _, None) => GuardState::NotScaffolded,
    };

    let cli_version = env!("CARGO_PKG_VERSION");
    let workflow = fs::read_optional(&dir.join(".github/workflows/visual-docs.yml"))?;
    let pin = match workflow.as_deref().map(preflight::workflow_pin) {
        None => PinState::NoWorkflow,
        Some(None) => PinState::NoPin,
        Some(Some(v)) if v == cli_version => PinState::Matches(v),
        Some(Some(pinned)) => PinState::Skew { pinned },
    };

    let docker = host::command_exists("docker");

    // Docker absence is advisory (you may capture elsewhere); the two ways a setup
    // silently lies — an unenabled guard and a version skew — are the real gate.
    let problems = matches!(guard, GuardState::PresentNotEnabled { .. })
        || matches!(pin, PinState::Skew { .. });

    match args.format {
        OutputFormat::Json => write_env_json(&guard, cli_version, &pin, docker, !problems, out)?,
        OutputFormat::Human if !ctx.quiet => {
            write_env_human(&guard, cli_version, &pin, docker, problems, out)?;
        }
        OutputFormat::Human => {}
    }

    if args.exit_code && problems {
        return Ok(3);
    }
    Ok(0)
}

/// Whether `hooks_path` (a `core.hooksPath` value, relative to `dir` or absolute)
/// resolves to the committed `.githooks` directory the scaffold uses.
fn points_at_githooks(dir: &Utf8Path, hooks_path: &str) -> bool {
    let p = Utf8Path::new(hooks_path);
    let resolved = if p.is_absolute() {
        p.to_owned()
    } else {
        dir.join(p)
    };
    resolved == dir.join(".githooks") || resolved.file_name() == Some(".githooks")
}

/// Stable single-line JSON contract for the environment preflight.
fn write_env_json(
    guard: &GuardState,
    cli_version: &str,
    pin: &PinState,
    docker: bool,
    ok: bool,
    out: &mut dyn Write,
) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Report<'a> {
        pre_push_guard: &'a str,
        hooks_path: Option<&'a str>,
        cli_version: &'a str,
        workflow_pin: &'a str,
        pinned_version: Option<&'a str>,
        docker: bool,
        ok: bool,
    }

    let (guard_kind, hooks_path) = match guard {
        GuardState::Enabled => ("enabled", None),
        GuardState::PresentNotEnabled { hooks_path } => {
            ("present-not-enabled", hooks_path.as_deref())
        }
        GuardState::Custom { hooks_path } => ("custom", Some(hooks_path.as_str())),
        GuardState::NotScaffolded => ("not-scaffolded", None),
    };
    let (pin_kind, pinned_version) = match pin {
        PinState::Matches(v) => ("matches", Some(v.as_str())),
        PinState::Skew { pinned } => ("skew", Some(pinned.as_str())),
        PinState::NoPin => ("no-pin", None),
        PinState::NoWorkflow => ("no-workflow", None),
    };

    let report = Report {
        pre_push_guard: guard_kind,
        hooks_path,
        cli_version,
        workflow_pin: pin_kind,
        pinned_version,
        docker,
        ok,
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// Human-readable preflight: one line per check, then a single verdict line.
fn write_env_human(
    guard: &GuardState,
    cli_version: &str,
    pin: &PinState,
    docker: bool,
    problems: bool,
    out: &mut dyn Write,
) -> Result<(), AppError> {
    let guard_line = match guard {
        GuardState::Enabled => "enabled (core.hooksPath -> .githooks)".to_owned(),
        GuardState::PresentNotEnabled { hooks_path: None } => {
            "PRESENT BUT NOT ENABLED — .githooks/pre-push exists but core.hooksPath is unset; \
             run: git config core.hooksPath .githooks"
                .to_owned()
        }
        GuardState::PresentNotEnabled {
            hooks_path: Some(p),
        } => format!(
            "PRESENT BUT NOT ENABLED — .githooks/pre-push exists but core.hooksPath is '{p}'; \
             run: git config core.hooksPath .githooks"
        ),
        GuardState::Custom { hooks_path } => {
            format!("custom (core.hooksPath = '{hooks_path}', no .githooks/pre-push)")
        }
        GuardState::NotScaffolded => "not set up — run: screencomp init --enable-hook".to_owned(),
    };
    writeln!(out, "pre-push guard: {guard_line}").map_err(write_err)?;

    writeln!(out, "cli version: {cli_version}").map_err(write_err)?;

    let pin_line = match pin {
        PinState::Matches(v) => format!("v{v} (matches this CLI)"),
        PinState::Skew { pinned } => format!(
            "v{pinned} pinned, but this CLI is v{cli_version} — SKEW; reinstall the CLI to \
             match, or re-run `screencomp init --force` to repin the workflow"
        ),
        PinState::NoPin => "workflow present but no recognizable @v<version> pin".to_owned(),
        PinState::NoWorkflow => {
            "no .github/workflows/visual-docs.yml — run: screencomp init".to_owned()
        }
    };
    writeln!(out, "workflow pin: {pin_line}").map_err(write_err)?;

    let docker_line = if docker {
        "available"
    } else {
        "not found — capture needs Docker; install it or capture on another machine"
    };
    writeln!(out, "docker: {docker_line}").map_err(write_err)?;

    if problems {
        writeln!(
            out,
            "problems found: the strict gate's local guard is not active"
        )
    } else {
        writeln!(out, "ok: environment ready")
    }
    .map_err(write_err)
}
