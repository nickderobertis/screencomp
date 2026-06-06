//! Filesystem access: discover a screenshot tree and write generated files.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest as _, Sha256};

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

/// Write `contents` to `path`, creating parent directories as needed.
pub(crate) fn write_string(path: &Utf8Path, contents: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| AppError::io(format!("creating {parent}"), e))?;
    }
    fs::write(path, contents).map_err(|e| AppError::io(format!("writing {path}"), e))
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
}
