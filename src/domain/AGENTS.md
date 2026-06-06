# AGENTS — domain

- Pure only: no `std::fs`, `std::env`, process, or printing. Take primitives and
  in-memory snapshots; return data.
- Deterministic output: iterate snapshots in `(project, name)` order so renders
  and listings are byte-stable.
- Do not depend on `cli`, `io`, `config`, or `commands`. Receive configuration
  as plain parameters, not config types.
