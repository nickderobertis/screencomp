# AGENTS — browser-tests

- Playwright in real Chromium over the gallery the freshly built CLI renders
  (`browser-test` depends on `screencomp-cli:build` and puts `target/debug` on PATH).
- Out of the gate (the gate stays browser-free); `test-visual-docs.yml`'s
  `gallery-browser` job runs it. Its dependencies resolve from the root
  `bun.lock`; `package.json` sets `nx.includedScripts: []` so no npm script
  becomes a gate target.
