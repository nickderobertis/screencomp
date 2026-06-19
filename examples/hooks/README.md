# Local pre-push guard (the strict gate's local half)

[`../pre-push`](../pre-push) is a copy-paste Git pre-push hook that re-captures
your screenshots **only when screenshot-relevant files change** and makes any
baseline change loud and reviewable before you push.

It is the local half of the **strict gate** (the recommended model): CI
hard-fails when a capture drifts from the committed baseline, and this hook lets
you regenerate and **commit** the new baseline before pushing — so CI stays green
on intended changes and goes red only on ones you missed. Without it (or under the
lighter CI-auto-accept model) you can change UI, pass your whole local gate, and
push without ever learning the visual baseline moved. The hook closes that gap on
your machine, before CI ever runs.

`screencomp init` scaffolds this hook at `.githooks/pre-push`; it detects the host
arch at runtime, so the one committed hook is correct on every developer's machine.
The steps below are for wiring the copy-paste template by hand or with a hook
manager.

## How it decides whether to run

The hook fires only when a pushed change matches the `[guard].paths` globs in
`screencomp.toml`:

```toml
[capture]
arches = ["x86_64"]   # arch(es) you maintain; the hook detects the host arch itself

[guard]
# Globs (matched against `git diff --name-only`, repo-root-relative) that should
# trigger a re-capture. Empty/omitted means the guard never fires.
paths = ["src/**/*.{ts,tsx,css}", "playwright/**", "public/**"]
manifest = "shots/baseline/x86_64.json"          # committed digest baseline
gallery  = "shots/review"                        # local review-gallery output dir
```

The relevance check is delegated to `screencomp scope`, so it is robust string
matching rather than fragile shell globbing:

```sh
git diff --name-only "$range" | screencomp scope --changed-from - --exit-code
# exit 3 -> a relevant path matched (capture);  exit 0 -> nothing relevant (skip)
```

Because matching nothing exits immediately with no Docker and no capture, the
common push is cheap. Only a relevant change pays the (deliberately slow,
container-backed) capture cost. Keep the `[guard]` values here in sync with the
shell variables at the top of `pre-push`.

## Behavior

- **Nothing relevant changed** → exit 0 silently.
- **Relevant change, but Docker unavailable** → loud warning, non-zero exit. A
  pass without a working capture environment would be false assurance.
- **Relevant change, in parity with the baseline** → one confirmation line, push
  proceeds.
- **Relevant change, baseline drifted** → the manifest is regenerated, a review
  gallery is built, and the push is **blocked** with instructions. The hook never
  auto-commits: you review the gallery, commit the regenerated manifest, and push
  again.

`git push --no-verify` bypasses the hook. Under CI (`$CI` set) it is a no-op.

## Wiring

Pick the manager you already use. Each runs the same script on `pre-push`.

### Committed `.githooks/` (what `init` scaffolds)

Commit the hook to a tracked directory and point Git at it — no hook manager
needed, and every clone enables it with one command:

```sh
mkdir -p .githooks
cp examples/pre-push .githooks/pre-push   # `screencomp init` writes this for you
chmod +x .githooks/pre-push
git config core.hooksPath .githooks       # once per clone
```

### Raw `.git/hooks/pre-push`

```sh
cp examples/pre-push .git/hooks/pre-push
chmod +x .git/hooks/pre-push
```

`.git/hooks/` is not version-controlled, so each clone must copy it (or set
`git config core.hooksPath` to a committed directory containing the script).

### lefthook (`lefthook.yml`)

```yaml
pre-push:
  commands:
    screencomp-guard:
      # Git's stdin (the push refs) must reach the script.
      run: bash examples/pre-push
      use_stdin: true
```

### husky

```sh
npx husky init
echo 'bash examples/pre-push' > .husky/pre-push
```

Husky forwards Git's stdin and arguments to the hook script, so `pre-push`
reads the push refs unchanged.

### simple-git-hooks (`package.json`)

```json
{
  "simple-git-hooks": {
    "pre-push": "bash examples/pre-push"
  }
}
```

Then run `npx simple-git-hooks` to install. simple-git-hooks invokes the command
with Git's stdin attached.

## Testing the hook without a real push

Set `SCREENCOMP_GUARD_RANGE` to a commit range to bypass the stdin ref parsing
and diff that range directly:

```sh
SCREENCOMP_GUARD_RANGE="origin/main..HEAD" bash examples/pre-push
```
