import { readFileSync } from 'node:fs';
const policy = JSON.parse(readFileSync(process.argv[2], 'utf8'));
for (const key of ['macosRequired', 'isReleaseOnly']) {
  if (typeof policy[key] !== 'boolean') throw new Error(`missing policy boolean ${key}`);
  console.log(`${key}=${policy[key]}`);
}
