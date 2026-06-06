# AGENTS — io

- The only place filesystem access is allowed; keep domain logic out of it.
- Paths are `camino` UTF-8; a non-UTF-8 entry is an error, not a lossy guess.
- Wrap failures in `AppError` with operation and path context.
- Layout convention is `<root>/<project>/<name>.png`; ignore non-`.png` files and
  top-level non-directories rather than failing.
