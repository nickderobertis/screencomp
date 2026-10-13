import { check } from '@onebudgetspec/sdk';
const result = await check({ paths: ['.github/budgets.yaml'], ids: ['screencomp-macos-jobs-per-run', 'screencomp-macos-runs-per-change'] });
for (const budget of result.results) {
  console.log(`${budget.id}: ${budget.verdict} (${budget.actual} ${budget.unit})`);
  if (budget.verdict !== 'within') { console.error(budget.error ?? budget.detail); process.exitCode = 1; }
}
