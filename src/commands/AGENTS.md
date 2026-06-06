# AGENTS — commands

- Each handler reads via `io`, computes via `domain`, and writes only to the
  passed writer. No `println!`/`eprintln!`; stderr belongs to `main`.
- Return `Result<i32, AppError>`. Exit `3` is success-with-differences, not an
  error, and only `classify --exit-code` returns it.
- Honor `Ctx.quiet` for human output; never gate machine output (`--format
  json`) on it.
