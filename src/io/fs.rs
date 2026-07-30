//! Filesystem access: read a capture index and write generated files.

use std::collections::BTreeSet;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::digest;
use crate::domain::index::CaptureIndex;
use crate::domain::naming;
use crate::domain::snapshot::Snapshot;
use crate::errors::AppError;

/// Filename of the capture index inside a capture directory. Carries each shot's
/// toggles, content hash, and image path (see [`crate::domain::index`]).
pub(crate) const CAPTURES_FILE: &str = "captures.json";

/// Read the capture index at `<dir>/captures.json` into a [`Snapshot`].
///
/// A missing `dir` is [`AppError::NotADirectory`] (so a wrong `--arch` subtree
/// fails identically to every other command, with a layout hint); a `dir` that
/// exists but holds no `captures.json` is an [`AppError::InvalidLayout`] naming
/// the expected file, since that is the wrong-path mistake a capture step makes.
pub(crate) fn discover(dir: &Utf8Path) -> Result<Snapshot, AppError> {
    if !dir.is_dir() {
        return Err(AppError::NotADirectory {
            path: dir.to_owned(),
        });
    }
    let index = dir.join(CAPTURES_FILE);
    if !index.is_file() {
        return Err(AppError::InvalidLayout {
            path: index,
            reason: format!(
                "missing the capture index '{CAPTURES_FILE}'; the capture step must write it \
                 alongside the screenshots"
            ),
        });
    }
    read_index_file(&index)
}

/// Read a baseline (or live) capture index file directly into a [`Snapshot`].
///
/// Used for `--baseline-manifest <file>`: a digest-only index written by
/// `screencomp manifest`. A missing file is an [`AppError::Io`]; a malformed one
/// is an [`AppError::InvalidLayout`] naming the reason.
pub(crate) fn read_manifest(path: &Utf8Path) -> Result<Snapshot, AppError> {
    read_index_file(path)
}

/// Parse a `captures.json`-shaped file at `path` into a [`Snapshot`].
fn read_index_file(path: &Utf8Path) -> Result<Snapshot, AppError> {
    let text = fs::read_to_string(path).map_err(|e| AppError::io(format!("reading {path}"), e))?;
    let index: CaptureIndex = serde_json::from_str(&text).map_err(|e| AppError::InvalidLayout {
        path: path.to_owned(),
        reason: e.to_string(),
    })?;
    index
        .into_snapshot()
        .map_err(|reason| AppError::InvalidLayout {
            path: path.to_owned(),
            reason,
        })
}

/// Every PNG beneath `dir`, as `/`-separated paths relative to `dir` in sorted
/// order.
///
/// The tree `index` turns into an index: directories are walked depth-first and
/// anything that is not a `.png` (the `captures.json` it is about to write, a
/// rendered gallery, a stray note) is ignored, so re-indexing a directory the tool
/// itself has written to is safe. Symlinked directories are not followed — a
/// capture is a plain tree, and following them risks a cycle. Sorting makes the
/// resulting index independent of directory-read order.
///
/// Segments are joined with `/` rather than the platform separator, since these
/// paths become the index's `image` values: one capture must describe the same
/// shots whether it was indexed on Linux or Windows.
pub(crate) fn find_pngs(dir: &Utf8Path) -> Result<Vec<String>, AppError> {
    if !dir.is_dir() {
        return Err(AppError::NotADirectory {
            path: dir.to_owned(),
        });
    }
    let mut found = Vec::new();
    collect_pngs(dir, "", &mut found)?;
    found.sort();
    Ok(found)
}

/// Append every PNG under `<root>/<prefix>` to `found`, relative to `root`.
fn collect_pngs(root: &Utf8Path, prefix: &str, found: &mut Vec<String>) -> Result<(), AppError> {
    let dir = if prefix.is_empty() {
        root.to_owned()
    } else {
        root.join(prefix)
    };
    let entries = fs::read_dir(&dir).map_err(|e| AppError::io(format!("reading {dir}"), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::io(format!("reading {dir}"), e))?;
        let name = entry.file_name();
        // A non-UTF-8 name cannot be recorded in the index (which is JSON text),
        // so it is a hard error rather than a lossy guess.
        let Some(name) = name.to_str() else {
            return Err(AppError::InvalidLayout {
                path: dir.clone(),
                reason: format!("entry {name:?} is not valid UTF-8"),
            });
        };
        let relative = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry
            .file_type()
            .map_err(|e| AppError::io(format!("inspecting {dir}/{name}"), e))?;
        if kind.is_dir() {
            collect_pngs(root, &relative, found)?;
        } else if naming::is_png(name) {
            found.push(relative);
        }
    }
    Ok(())
}

/// Hex SHA-256 of the file at `path` — the digest a `captures.json` entry records.
pub(crate) fn hash_file(path: &Utf8Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(format!("reading {path}"), e))?;
    Ok(digest::hex_sha256(&bytes))
}

/// Image paths in `snapshot` whose PNG is absent under `dir`, sorted and unique.
///
/// `doctor` uses this to flag an index that references screenshots the capture
/// step never wrote — a silently broken gallery. Digest-only shots (no image) are
/// skipped, since a baseline intentionally commits no PNGs.
pub(crate) fn missing_images(dir: &Utf8Path, snapshot: &Snapshot) -> Vec<String> {
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for (_key, shot) in snapshot.iter() {
        if let Some(image) = shot.image.as_deref()
            && !dir.join(image).is_file()
        {
            missing.insert(image.to_owned());
        }
    }
    missing.into_iter().collect()
}

/// Copy every image referenced by `snapshot` from `src_dir` into `output`,
/// preserving each image's relative path, so a rendered gallery is self-contained
/// and deploy-ready. Returns the number of distinct images copied. A referenced
/// image that is absent is an [`AppError::Io`].
pub(crate) fn copy_images(
    src_dir: &Utf8Path,
    output: &Utf8Path,
    snapshot: &Snapshot,
) -> Result<usize, AppError> {
    let images: BTreeSet<&str> = snapshot
        .iter()
        .filter_map(|(_key, shot)| shot.image.as_deref())
        .collect();

    for image in &images {
        let src = src_dir.join(image);
        let dest = output.join(image);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("creating {parent}"), e))?;
        }
        fs::copy(&src, &dest).map_err(|e| AppError::io(format!("copying {src} to {dest}"), e))?;
    }
    Ok(images.len())
}

/// Copy a capture's source index into `output`, preserving its bytes.
pub(crate) fn copy_index(src_dir: &Utf8Path, output: &Utf8Path) -> Result<(), AppError> {
    let src = src_dir.join(CAPTURES_FILE);
    let dest = output.join(CAPTURES_FILE);
    fs::create_dir_all(output).map_err(|e| AppError::io(format!("creating {output}"), e))?;
    fs::copy(&src, &dest).map_err(|e| AppError::io(format!("copying {src} to {dest}"), e))?;
    Ok(())
}

/// Read `path` into a string, wrapping a failure as an [`AppError::Io`].
///
/// Used by `scope` to read a newline-delimited candidate-path list from a file
/// (the stdin case is handled at the command boundary, like other process I/O).
pub(crate) fn read_text(path: &Utf8Path) -> Result<String, AppError> {
    fs::read_to_string(path).map_err(|e| AppError::io(format!("reading {path}"), e))
}

/// Read `path` into a string, treating a missing file as `Ok(None)`.
///
/// `doctor --env` inspects optional repository files (a scaffolded workflow that
/// may not exist yet), so absence is a normal state to report, not an error,
/// while any other failure (permissions, non-UTF-8) still surfaces.
pub(crate) fn read_optional(path: &Utf8Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(format!("reading {path}"), e)),
    }
}

/// Whether `path` is an existing regular file. A presence probe for the
/// environment preflight (e.g. "is the committed pre-push hook there?").
pub(crate) fn file_exists(path: &Utf8Path) -> bool {
    path.is_file()
}

/// Walk from `start` up through its ancestors, returning the first existing
/// `<dir>/<filename>`.
///
/// Used to auto-discover `screencomp.toml` when no config path is given
/// explicitly, mirroring how `cargo`/`rustfmt` locate their config from any
/// subdirectory. The nearest file wins (`start` is checked before its parents).
pub(crate) fn find_up(start: &Utf8Path, filename: &str) -> Option<Utf8PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(filename))
        .find(|candidate| candidate.is_file())
}

/// Write `contents` to `path`, creating parent directories as needed.
pub(crate) fn write_string(path: &Utf8Path, contents: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| AppError::io(format!("creating {parent}"), e))?;
    }
    fs::write(path, contents).map_err(|e| AppError::io(format!("writing {path}"), e))
}

/// What a scaffold write did to a file (`screencomp init`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scaffold {
    /// The file did not exist and was written.
    Created,
    /// The file existed and was left untouched (no `--force`).
    Skipped,
    /// The file existed and was overwritten (`--force`).
    Overwritten,
}

/// Write a scaffold file without clobbering existing work unless `force` is set.
///
/// Returns whether the file was created, skipped, or overwritten so `init` can
/// report each path and stay safe to re-run.
pub(crate) fn write_scaffold(
    path: &Utf8Path,
    contents: &str,
    force: bool,
) -> Result<Scaffold, AppError> {
    let existed = path.exists();
    if existed && !force {
        return Ok(Scaffold::Skipped);
    }
    write_string(path, contents)?;
    Ok(if existed {
        Scaffold::Overwritten
    } else {
        Scaffold::Created
    })
}

/// Like [`write_scaffold`], but also marks the file executable on Unix.
///
/// Used for the scaffolded pre-push hook: a Git hook (whether under
/// `core.hooksPath` or `.git/hooks`) must be executable to run at all. The bit is
/// a no-op concept on Windows, where Git decides executability differently, so it
/// is set only on Unix and a skipped (already-present) file is left untouched.
pub(crate) fn write_executable_scaffold(
    path: &Utf8Path,
    contents: &str,
    force: bool,
) -> Result<Scaffold, AppError> {
    let outcome = write_scaffold(path, contents, force)?;
    #[cfg(unix)]
    if outcome != Scaffold::Skipped {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(path)
            .map_err(|e| AppError::io(format!("reading {path}"), e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| AppError::io(format!("setting mode on {path}"), e))?;
    }
    Ok(outcome)
}

/// Append `block` to the `.gitignore` at `path`, idempotently.
///
/// `block` is added only when `marker` (a sentinel line within it) is not already
/// present, so re-running `init` never duplicates the entries. A newline is
/// inserted first when the existing file does not end in one. Returns the
/// scaffold outcome: `Created` for a new file, `Overwritten` for an append, or
/// `Skipped` when the marker is already there.
pub(crate) fn append_block(
    path: &Utf8Path,
    block: &str,
    marker: &str,
) -> Result<Scaffold, AppError> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(AppError::io(format!("reading {path}"), e)),
    };

    match existing {
        Some(text) if text.contains(marker) => Ok(Scaffold::Skipped),
        Some(text) => {
            let mut next = text;
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(block);
            write_string(path, &next)?;
            Ok(Scaffold::Overwritten)
        }
        None => {
            write_string(path, block)?;
            Ok(Scaffold::Created)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::{Shot, ShotKey};

    /// Write a `captures.json` under `dir` with the given shots and return `dir`.
    fn write_index(dir: &Utf8Path, shots: &[(ShotKey, &str, Option<&str>)]) {
        let entries: Vec<String> = shots
            .iter()
            .map(|(key, hash, image)| {
                let toggles: Vec<String> = key
                    .toggles
                    .iter()
                    .map(|(k, v)| format!("\"{k}\":\"{v}\""))
                    .collect();
                let image = image
                    .map(|i| format!(",\"image\":\"{i}\""))
                    .unwrap_or_default();
                format!(
                    "{{\"name\":\"{}\",\"toggles\":{{{}}},\"hash\":\"{hash}\"{image}}}",
                    key.name,
                    toggles.join(",")
                )
            })
            .collect();
        let json = format!("{{\"schema\":1,\"shots\":[{}]}}", entries.join(","));
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(CAPTURES_FILE), json).unwrap();
    }

    #[test]
    fn discover_missing_dir_is_not_a_directory() {
        let err = discover(Utf8Path::new("/no/such/dir")).unwrap_err();
        assert!(matches!(err, AppError::NotADirectory { .. }));
    }

    #[test]
    fn discover_without_index_is_invalid_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        let err = discover(dir).unwrap_err();
        let AppError::InvalidLayout { reason, .. } = err else {
            panic!("expected InvalidLayout, got {err:?}");
        };
        assert!(reason.contains(CAPTURES_FILE), "{reason}");
    }

    #[test]
    fn discover_reads_shots() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        let hash = "a".repeat(64);
        write_index(
            dir,
            &[(
                ShotKey::with("home", &[("theme", "dark")]),
                &hash,
                Some("home.png"),
            )],
        );
        let snap = discover(dir).unwrap();
        assert_eq!(
            snap.digest(&ShotKey::with("home", &[("theme", "dark")])),
            Some(hash.as_str())
        );
    }

    #[test]
    fn find_pngs_walks_recursively_ignoring_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        fs::write(dir.join("home/mobile.png"), b"m").unwrap();
        fs::write(dir.join("home/desktop.PNG"), b"d").unwrap();
        fs::write(dir.join("about.png"), b"a").unwrap();
        // Not screenshots: the index this walk feeds, a rendered gallery, a note.
        fs::write(dir.join(CAPTURES_FILE), b"{}").unwrap();
        fs::write(dir.join("index.html"), b"<html>").unwrap();
        fs::write(dir.join("notes"), b"x").unwrap();

        let found = find_pngs(dir).unwrap();
        assert_eq!(
            found,
            vec![
                "about.png".to_owned(),
                // `/`-joined on every platform, since these become index paths.
                "home/desktop.PNG".to_owned(),
                "home/mobile.png".to_owned(),
            ],
            "sorted, relative, PNGs only"
        );
    }

    #[test]
    fn find_pngs_missing_dir_is_not_a_directory() {
        assert!(matches!(
            find_pngs(Utf8Path::new("/no/such/capture")).unwrap_err(),
            AppError::NotADirectory { .. }
        ));
    }

    #[test]
    fn hash_file_is_the_hex_sha256_of_the_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(tmp.path()).unwrap().join("shot.png");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hash_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(matches!(
            hash_file(&path.with_file_name("gone.png")).unwrap_err(),
            AppError::Io { .. }
        ));
    }

    #[test]
    fn read_manifest_missing_file_is_io_error() {
        let err = read_manifest(Utf8Path::new("/no/such/baseline.json")).unwrap_err();
        assert!(matches!(err, AppError::Io { .. }));
    }

    #[test]
    fn read_manifest_malformed_is_invalid_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(tmp.path()).unwrap().join("b.json");
        fs::write(&path, "{not json").unwrap();
        assert!(matches!(
            read_manifest(&path).unwrap_err(),
            AppError::InvalidLayout { .. }
        ));
    }

    #[test]
    fn missing_images_lists_absent_pngs_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        fs::write(dir.join("there.png"), b"x").unwrap();
        let mut snap = Snapshot::new();
        snap.insert(
            ShotKey::bare("a"),
            Shot::new("aa", Some("there.png".to_owned())),
        );
        snap.insert(
            ShotKey::bare("b"),
            Shot::new("bb", Some("gone.png".to_owned())),
        );
        snap.insert(ShotKey::bare("c"), Shot::new("cc", None)); // digest-only, skipped
        assert_eq!(missing_images(dir, &snap), vec!["gone.png".to_owned()]);
    }

    #[test]
    fn copy_images_reproduces_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        let input = root.join("in");
        let output = root.join("out");
        fs::create_dir_all(input.join("home")).unwrap();
        fs::write(input.join("home/dark.png"), b"png-bytes").unwrap();

        let mut snap = Snapshot::new();
        snap.insert(
            ShotKey::with("home", &[("theme", "dark")]),
            Shot::new("aa", Some("home/dark.png".to_owned())),
        );
        let copied = copy_images(&input, &output, &snap).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            fs::read(output.join("home/dark.png")).unwrap(),
            b"png-bytes"
        );
    }

    #[test]
    fn copy_images_missing_source_is_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        let mut snap = Snapshot::new();
        snap.insert(
            ShotKey::bare("home"),
            Shot::new("aa", Some("gone.png".to_owned())),
        );
        assert!(matches!(
            copy_images(&root.join("in"), &root.join("out"), &snap).unwrap_err(),
            AppError::Io { .. }
        ));
    }

    #[test]
    fn find_up_returns_the_nearest_ancestor_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("screencomp.toml"), b"root").unwrap();
        fs::write(root.join("a/screencomp.toml"), b"closer").unwrap();

        let found = find_up(&nested, "screencomp.toml").expect("walks up to a match");
        assert_eq!(found, root.join("a/screencomp.toml"));
    }

    #[test]
    fn find_up_returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = Utf8Path::from_path(tmp.path()).unwrap().join("x/y");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_up(&nested, "screencomp.toml"), None);
    }
}
