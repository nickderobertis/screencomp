import { readFileSync } from 'node:fs';
const args = process.argv.slice(2);
if (args.length !== 1 || typeof args[0] !== 'string' || !args[0].trim() || args[0].includes('\0')) {
  throw new Error('usage: node .github/macos-output.mjs <policy-json-path>');
}
const policy = JSON.parse(readFileSync(args[0], 'utf8'));
if (!policy || typeof policy !== 'object' || !['macosRequired', 'isReleaseOnly'].every(k => typeof policy[k] === 'boolean')) throw new Error('missing policy booleans; rerun the classifier');
for (const key of ['macosRequired', 'isReleaseOnly']) {
  console.log(`${key}=${policy[key]}`);
}
