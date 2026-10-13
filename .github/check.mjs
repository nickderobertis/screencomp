// Syntax checks are host-independent; workflow/shell lint stays on Linux.
import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
const files = ['.github', '.github/tests'].flatMap(dir => readdirSync(dir).filter(f => f.endsWith('.mjs')).map(f => `${dir}/${f}`));
for (const file of files) {
  const result = spawnSync(process.execPath, ['--check', file], { stdio: 'inherit' });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
