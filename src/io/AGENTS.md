# AGENTS — io

- The only place filesystem access is allowed; keep domain logic out of it.
- Paths are `camino` UTF-8; a non-UTF-8 entry is an error, not a lossy guess.
- Wrap failures in `AppError` with operation and path context.
- Spawn `git` only through `git_in`, which clears Git's repository-locating
  environment (`git rev-parse --local-env-vars`: `GIT_DIR`, `GIT_WORK_TREE`, …).
  A child `git` honours those over `-C <dir>`, and Git exports them to every
  hook it runs — so without clearing them a command invoked from a hook acts on
  the hook's repository instead of the one the user named.
- A capture is a directory holding `captures.json` (the index) plus the PNGs it
  references by relative path. A missing directory is `NotADirectory`; a directory
  without `captures.json`, or a malformed index, is `InvalidLayout`.
- `hash_file` (used only by `index`) is the single place image bytes are read. No
  other reader may open a PNG: every other command compares the hash the index
  records, and re-deriving one anywhere else would make the index stop being the
  source of truth.
