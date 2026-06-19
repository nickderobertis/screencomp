# AGENTS — io

- The only place filesystem access is allowed; keep domain logic out of it.
- Paths are `camino` UTF-8; a non-UTF-8 entry is an error, not a lossy guess.
- Wrap failures in `AppError` with operation and path context.
- A capture is a directory holding `captures.json` (the index) plus the PNGs it
  references by relative path. A missing directory is `NotADirectory`; a directory
  without `captures.json`, or a malformed index, is `InvalidLayout`.
