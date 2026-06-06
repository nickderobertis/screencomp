# Test fixtures

`baseline/` and `current/` are two screenshot trees laid out as
`<project>/<name>.png`. The `.png` files are **opaque byte blobs**, not rendered
images — the CLI only content-hashes bytes, so fixtures stay tiny and
human-diffable.

Comparing `current` against `baseline` yields:

| status    | screenshot         |
| --------- | ------------------ |
| changed   | `desktop/about`    |
| added     | `desktop/pricing`  |
| unchanged | `desktop/home`     |
| unchanged | `mobile/home`      |

Keep these in sync with the expectations asserted in `../integration.rs` and
`../e2e.rs`.
