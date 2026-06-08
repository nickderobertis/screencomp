//! Filesystem access: discover a screenshot tree and write generated files.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest as _, Sha256};

use crate::domain::layout::{LayoutScan, ProjectScan};
use crate::domain::snapshot::{ShotKey, Snapshot};
use crate::errors::AppError;

/// Walk `<root>/<project>/<name>.png` and build a content-addressed [`Snapshot`].
///
/// Non-directory entries at the project level and non-`.png` files within a
/// project are ignored, so the tree may coexist with other artifacts.
pub(crate) fn discover(root: &Utf8Path) -> Result<Snapshot, AppError> {
    if !root.is_dir() {
        return Err(AppError::NotADirectory {
            path: root.to_owned(),
        });
    }

    let mut snapshot = Snapshot::new();

    for project_dir in read_dir_sorted(root)? {
        if !project_dir.is_dir() {
            continue;
        }
        let Some(project) = project_dir.file_name() else {
            continue;
        };
        let project = project.to_owned();

        for file in read_dir_sorted(&project_dir)? {
            if !file.is_file() || !is_png(&file) {
                continue;
            }
            let Some(name) = file.file_stem() else {
                continue;
            };
            let bytes = fs::read(&file).map_err(|e| AppError::io(format!("reading {file}"), e))?;
            snapshot.insert(
                ShotKey {
                    project: project.clone(),
                    name: name.to_owned(),
                },
                digest_hex(&bytes),
            );
        }
    }

    Ok(snapshot)
}

/// Scan `<root>` one level deep into a [`LayoutScan`] describing its shape.
///
/// Unlike [`discover`], which content-hashes shots and discards everything that
/// is not a `<project>/<name>.png`, this records the structure a preflight needs:
/// each project directory with its `.png` count, and any `.png` files stranded
/// directly under the root (a common capture-path mistake that every command
/// then silently ignores). It reads directory entries but no file *bytes*, so it
/// stays cheap even on a large tree. A missing root is the same typed error
/// [`discover`] raises, so `doctor` reports it identically.
pub(crate) fn scan_layout(root: &Utf8Path) -> Result<LayoutScan, AppError> {
    if !root.is_dir() {
        return Err(AppError::NotADirectory {
            path: root.to_owned(),
        });
    }

    let mut projects = Vec::new();
    let mut loose_pngs = Vec::new();

    for entry in read_dir_sorted(root)? {
        if entry.is_dir() {
            let Some(name) = entry.file_name() else {
                continue;
            };
            let shots = read_dir_sorted(&entry)?
                .iter()
                .filter(|f| f.is_file() && is_png(f))
                .count();
            projects.push(ProjectScan {
                name: name.to_owned(),
                shots,
            });
        } else if entry.is_file()
            && is_png(&entry)
            && let Some(name) = entry.file_name()
        {
            loose_pngs.push(name.to_owned());
        }
    }

    // read_dir_sorted already yields entries in lexical order, so both lists are
    // sorted without an extra pass.
    Ok(LayoutScan {
        projects,
        loose_pngs,
    })
}

/// Read a digest manifest into a content-addressed [`Snapshot`].
///
/// Parses the `sha256sum`-style format produced by
/// [`crate::domain::manifest::render_manifest`]: one `<hex>  <project>/<name>.png`
/// line per shot. Blank lines are ignored; any other malformation is a typed
/// [`AppError::InvalidLayout`] naming the offending line, so a hand-edited or
/// truncated manifest fails loudly rather than silently dropping shots.
pub(crate) fn read_manifest(path: &Utf8Path) -> Result<Snapshot, AppError> {
    let text = fs::read_to_string(path).map_err(|e| AppError::io(format!("reading {path}"), e))?;

    let mut snapshot = Snapshot::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, digest) =
            parse_manifest_line(line).map_err(|reason| AppError::InvalidLayout {
                path: path.to_owned(),
                reason: format!("line {}: {reason}", index + 1),
            })?;
        snapshot.insert(key, digest);
    }

    Ok(snapshot)
}

/// Parse one `<hex>  <project>/<name>.png` manifest line into a key and digest.
///
/// Returns a human-readable reason on malformation; the caller adds path/line
/// context. Kept here (not in `domain`) so all manifest parsing lives at the I/O
/// boundary alongside the file read.
fn parse_manifest_line(line: &str) -> Result<(ShotKey, String), String> {
    let (digest, rest) = line
        .split_once(char::is_whitespace)
        .ok_or("expected '<digest>  <project>/<name>.png'")?;
    let path = rest.trim_start();

    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("'{digest}' is not a 64-character hex digest"));
    }

    let stem = path
        .strip_suffix(".png")
        .ok_or_else(|| format!("path '{path}' does not end in .png"))?;
    let (project, name) = stem
        .split_once('/')
        .ok_or_else(|| format!("path '{path}' is not '<project>/<name>.png'"))?;
    if project.is_empty() || name.is_empty() || name.contains('/') {
        return Err(format!("path '{path}' is not '<project>/<name>.png'"));
    }

    Ok((
        ShotKey {
            project: project.to_owned(),
            name: name.to_owned(),
        },
        digest.to_owned(),
    ))
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

/// Copy every `<project>/<name>.png` under `input` into the matching path under
/// `output`, so a rendered gallery is self-contained and deploy-ready. Returns
/// the number of images copied. Mirrors [`discover`]'s traversal and naming.
pub(crate) fn copy_png_tree(input: &Utf8Path, output: &Utf8Path) -> Result<usize, AppError> {
    if !input.is_dir() {
        return Err(AppError::NotADirectory {
            path: input.to_owned(),
        });
    }

    let mut copied = 0usize;
    for project_dir in read_dir_sorted(input)? {
        if !project_dir.is_dir() {
            continue;
        }
        let Some(project) = project_dir.file_name() else {
            continue;
        };

        for file in read_dir_sorted(&project_dir)? {
            if !file.is_file() || !is_png(&file) {
                continue;
            }
            let Some(stem) = file.file_stem() else {
                continue;
            };
            let dest_dir = output.join(project);
            fs::create_dir_all(&dest_dir)
                .map_err(|e| AppError::io(format!("creating {dest_dir}"), e))?;
            // Match the `<name>.png` reference render_html emits.
            let dest = dest_dir.join(format!("{stem}.png"));
            fs::copy(&file, &dest)
                .map_err(|e| AppError::io(format!("copying {file} to {dest}"), e))?;
            copied += 1;
        }
    }

    Ok(copied)
}

/// Read a directory into UTF-8 paths sorted lexically; non-UTF-8 names are an error.
fn read_dir_sorted(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>, AppError> {
    let entries =
        fs::read_dir(dir).map_err(|e| AppError::io(format!("reading directory {dir}"), e))?;

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| AppError::io(format!("reading directory {dir}"), e))?;
        let path =
            Utf8PathBuf::from_path_buf(entry.path()).map_err(|raw| AppError::InvalidLayout {
                path: Utf8PathBuf::from(raw.to_string_lossy().into_owned()),
                reason: "path is not valid UTF-8".to_owned(),
            })?;
        paths.push(path);
    }

    paths.sort();
    Ok(paths)
}

/// Whether `path` has a `.png` extension (case-insensitive).
fn is_png(path: &Utf8Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

/// Lowercase hex SHA-256 of `bytes`.
fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_root_is_not_a_directory() {
        let err = discover(Utf8Path::new("/no/such/dir")).unwrap_err();
        assert!(matches!(err, AppError::NotADirectory { .. }));
    }

    #[test]
    fn digest_is_stable_and_distinct() {
        assert_eq!(digest_hex(b"abc"), digest_hex(b"abc"));
        assert_ne!(digest_hex(b"abc"), digest_hex(b"abd"));
        // Known SHA-256 of "abc".
        assert_eq!(
            digest_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn png_detection_is_case_insensitive() {
        assert!(is_png(Utf8Path::new("a/b.png")));
        assert!(is_png(Utf8Path::new("a/b.PNG")));
        assert!(!is_png(Utf8Path::new("a/b.jpg")));
    }

    #[test]
    fn copy_png_tree_reproduces_the_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        let input = root.join("in");
        let output = root.join("out");
        fs::create_dir_all(input.join("desktop")).unwrap();
        fs::write(input.join("desktop/home.png"), b"png-bytes").unwrap();
        // A non-png sibling must be ignored.
        fs::write(input.join("desktop/notes.txt"), b"skip").unwrap();

        let copied = copy_png_tree(&input, &output).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            fs::read(output.join("desktop/home.png")).unwrap(),
            b"png-bytes"
        );
        assert!(!output.join("desktop/notes.txt").exists());
    }

    #[test]
    fn scan_layout_reports_projects_loose_pngs_and_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        fs::create_dir_all(root.join("desktop")).unwrap();
        fs::create_dir_all(root.join("mobile")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("desktop/home.png"), b"a").unwrap();
        fs::write(root.join("desktop/about.png"), b"b").unwrap();
        fs::write(root.join("desktop/notes.txt"), b"skip").unwrap(); // non-png ignored
        fs::write(root.join("mobile/home.png"), b"c").unwrap();
        // A PNG stranded at the root: the layout mistake the scan exists to catch.
        fs::write(root.join("stray.png"), b"d").unwrap();
        fs::write(root.join("README.md"), b"e").unwrap(); // non-png at root ignored

        let scan = scan_layout(root).unwrap();
        assert_eq!(
            scan.projects,
            vec![
                ProjectScan {
                    name: "desktop".to_owned(),
                    shots: 2,
                },
                ProjectScan {
                    name: "empty".to_owned(),
                    shots: 0,
                },
                ProjectScan {
                    name: "mobile".to_owned(),
                    shots: 1,
                },
            ]
        );
        assert_eq!(scan.loose_pngs, vec!["stray.png".to_owned()]);
        assert_eq!(scan.total_shots(), 3);
        assert!(scan.has_problems()); // the loose PNG
    }

    #[test]
    fn scan_layout_missing_root_is_not_a_directory() {
        let err = scan_layout(Utf8Path::new("/no/such/dir")).unwrap_err();
        assert!(matches!(err, AppError::NotADirectory { .. }));
    }

    #[test]
    fn read_manifest_roundtrips_render() {
        use crate::domain::manifest::render_manifest;

        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(tmp.path()).unwrap().join("b.sha256");
        let digest = "a".repeat(64);
        fs::write(
            &path,
            format!("{digest}  desktop/home.png\n{digest}  mobile/home.png\n"),
        )
        .unwrap();

        let snap = read_manifest(&path).unwrap();
        // Blank lines tolerated, order normalized, and a re-render matches input.
        assert_eq!(
            render_manifest(&snap),
            format!("{digest}  desktop/home.png\n{digest}  mobile/home.png\n")
        );
    }

    #[test]
    fn read_manifest_tolerates_blank_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(tmp.path()).unwrap().join("b.sha256");
        let digest = "b".repeat(64);
        fs::write(&path, format!("\n{digest}  desktop/home.png\n\n")).unwrap();
        assert_eq!(read_manifest(&path).unwrap().keys().count(), 1);
    }

    #[test]
    fn read_manifest_rejects_malformed_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();

        let hex = "c".repeat(64);
        let cases = [
            "notadigest  desktop/home.png".to_owned(), // bad digest
            format!("{hex}  desktop/home.txt"),        // not .png
            format!("{hex}  home.png"),                // no project/
            hex.clone(),                               // no path at all
        ];
        for (i, body) in cases.iter().enumerate() {
            let path = dir.join(format!("m{i}.sha256"));
            fs::write(&path, body).unwrap();
            let err = read_manifest(&path).unwrap_err();
            assert!(
                matches!(err, AppError::InvalidLayout { .. }),
                "case {i} ({body}) should be InvalidLayout, got {err:?}"
            );
        }
    }

    #[test]
    fn read_manifest_missing_file_is_io_error() {
        let err = read_manifest(Utf8Path::new("/no/such/baseline.sha256")).unwrap_err();
        assert!(matches!(err, AppError::Io { .. }));
    }
}
