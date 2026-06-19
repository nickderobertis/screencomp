# Test fixtures

`baseline/` and `current/` are two captures. Each is a directory holding a
`captures.json` index plus the PNG image files it references. The index records,
per shot, a base `name`, a `toggles` map (here a single `viewport` dimension), the
hex SHA-256 `hash` of the PNG bytes, and the `image` path relative to the index.
The `.png` files are **opaque byte blobs**, not rendered images — the CLI only
reads the hash from `captures.json` and never decodes pixels, so fixtures stay
tiny and human-diffable.

The two indexes hold:

| shot                       | image                | baseline hash | current hash  |
| -------------------------- | -------------------- | ------------- | ------------- |
| `about [viewport=desktop]` | `about-desktop.png`  | `1111…`       | `2222…`       |
| `home [viewport=desktop]`  | `home-desktop.png`   | `3333…`       | `3333…`       |
| `home [viewport=mobile]`   | `home-mobile.png`    | `4444…`       | `4444…`       |
| `pricing [viewport=desktop]` | `pricing-desktop.png` | (absent)    | `5555…`       |

Comparing `current` against `baseline` therefore yields:

| status    | shot                         |
| --------- | ---------------------------- |
| changed   | `about [viewport=desktop]`   |
| added     | `pricing [viewport=desktop]` |
| unchanged | `home [viewport=desktop]`    |
| unchanged | `home [viewport=mobile]`     |

i.e. `added 1 changed 1 removed 0 unchanged 2`.

Keep these in sync with the expectations asserted in `../integration.rs` and
`../e2e.rs`.
