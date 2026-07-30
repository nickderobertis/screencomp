//! `screencomp index` — author the `captures.json` index for a tree of PNGs.
//!
//! Every capture step has to write that index, and each one hand-rolls the same
//! two steps: hash each PNG, then record its name, toggles, and relative path.
//! This command *is* those two steps, so adopting the format costs one line in a
//! capture script. It changes nothing about the trust model: the capture side
//! still owns producing the digests, and every other command still treats a
//! recorded hash as the source of truth and never re-computes it.

use std::io::Write;

use camino::Utf8Path;

use super::{Ctx, arch, resolve_arch, write_err};
use crate::cli::IndexArgs;
use crate::domain::index::render_capture;
use crate::domain::naming::{self, Naming};
use crate::domain::snapshot::{Shot, Snapshot, Toggles};
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &IndexArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let arch = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let dir = arch::scope(&args.input, arch.as_deref());

    let images = fs::find_pngs(&dir).map_err(|e| hint_missing_subtree(e, &args.input, &dir))?;
    if images.is_empty() {
        return Err(AppError::InvalidLayout {
            path: dir,
            reason: "no .png files to index; the capture step wrote no screenshots here".to_owned(),
        });
    }

    let naming = Naming {
        fixed: fixed_toggles(args)?,
        toggles_from_path: args.toggles_from_path,
    };

    let mut snapshot = Snapshot::new();
    for image in &images {
        let key = naming::shot_key(image, &naming).map_err(|reason| AppError::InvalidLayout {
            path: dir.join(image),
            reason,
        })?;
        // Two paths can name one shot (`a/theme=dark.png` and `theme=dark/a.png`),
        // and silently keeping the last would drop a screenshot from the index.
        if snapshot.get(&key).is_some() {
            return Err(AppError::InvalidLayout {
                path: dir.join(image),
                reason: format!(
                    "shot '{}' is already indexed from another path; two screenshots \
                     cannot share one name and toggle map",
                    key.label()
                ),
            });
        }
        let hash = fs::hash_file(&dir.join(image))?;
        snapshot.insert(key, Shot::new(hash, Some(image.clone())));
    }

    let path = dir.join(fs::CAPTURES_FILE);
    fs::write_string(&path, &render_capture(&snapshot))?;
    if !ctx.quiet {
        writeln!(out, "wrote {path} ({} shots)", images.len()).map_err(write_err)?;
    }
    Ok(0)
}

/// The `--toggle KEY=VALUE` assignments every shot gets.
///
/// One key given twice is a usage mistake, not a last-one-wins silent choice: the
/// index it would produce describes neither pass the user meant.
fn fixed_toggles(args: &IndexArgs) -> Result<Toggles, AppError> {
    let mut fixed = Toggles::new();
    for toggle in &args.toggles {
        if let Some(existing) = fixed.insert(toggle.key().to_owned(), toggle.value().to_owned())
            && existing != toggle.value()
        {
            return Err(AppError::InvalidLayout {
                path: args.input.clone(),
                reason: format!(
                    "--toggle sets '{}' twice with different values ('{existing}' and '{}')",
                    toggle.key(),
                    toggle.value()
                ),
            });
        }
    }
    Ok(fixed)
}

/// Explain a missing arch subtree, the one layout mistake `index` can diagnose.
///
/// With `[capture].arches` configured, `--input shots/current` resolves to
/// `shots/current/<arch>/`; a capture that wrote its PNGs flat at the root
/// otherwise fails with a bare "not a directory" that hides the arch layer.
/// Everything else (a genuinely absent root, an I/O failure) passes through.
fn hint_missing_subtree(err: AppError, root: &Utf8Path, scoped: &Utf8Path) -> AppError {
    let AppError::NotADirectory { path } = &err else {
        return err;
    };
    if scoped == root || !root.is_dir() {
        return err;
    }
    AppError::InvalidLayout {
        path: path.clone(),
        reason: format!(
            "expected the arch subtree {scoped}, where screencomp would write \
             {file}; {root} exists but has no such subtree — have the capture step \
             write its PNGs under {scoped}/, or pass --arch to name the subtree it \
             did use",
            file = fs::CAPTURES_FILE,
        ),
    }
}
