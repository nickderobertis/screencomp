import { readFileSync } from 'node:fs';
const policy = JSON.parse(readFileSync(process.argv[2], 'utf8'));
if (!policy || typeof policy !== 'object' || !['macosRequired', 'isReleaseOnly'].every(k => typeof policy[k] === 'boolean')) throw new Error('missing policy booleans; rerun the classifier');
for (const key of ['macosRequired', 'isReleaseOnly']) {
  console.log(`${key}=${policy[key]}`);
}
