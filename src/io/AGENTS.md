# AGENTS — io

- The only place filesystem access is allowed; keep domain logic out of it.
- Paths are `camino` UTF-8; a non-UTF-8 entry is an error, not a lossy guess.
- Wrap failures in `AppError` with operation and path context.
- A capture is a directory holding `captures.json` (the index) plus the PNGs it
  references by relative path. A missing directory is `NotADirectory`; a directory
  without `captures.json`, or a malformed index, is `InvalidLayout`.
- `hash_file` (used only by `index`) is the single place image bytes are read. No
  other reader may open a PNG: every other command compares the hash the index
  records, and re-deriving one anywhere else would make the index stop being the
  source of truth.
