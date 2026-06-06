# AGENTS — src

- Dependency direction: `cli` → `commands` → {`domain`, `io`}. `domain` depends
  on nothing else in the crate; `io` depends only on `domain` types and
  `errors`. Never invert these arrows.
- `pub(crate)` by default. The crate's public API is only `run`, `Cli` (and its
  arg tree), `AppError`, and `ConfigError`.
- Read environment variables and config files only at a command boundary, never
  scattered through modules.
