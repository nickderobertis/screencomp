const eventName = process.env.GITHUB_EVENT_NAME;
const manual = process.env.MANUAL_MACOS ?? 'false';
if (!['pull_request', 'push', 'workflow_dispatch'].includes(eventName)) throw new Error(`unknown CI event ${eventName}; restore the GitHub event name before reporting an omission`);
if (!['true', 'false'].includes(manual) || (manual === 'true' && eventName !== 'workflow_dispatch')) throw new Error('invalid manual diagnostic value; use true only for explicit workflow_dispatch');
const needs = JSON.parse(process.env.NEEDS_JSON ?? 'null');
if (!needs || needs.policy?.result !== 'success') throw new Error('Mac policy prerequisite failed, cancelled or missing');
const required = needs.policy.outputs?.macosRequired;
const releaseOnly = needs.policy.outputs?.isReleaseOnly;
if (!['true', 'false'].includes(required) || !['true', 'false'].includes(releaseOnly)) throw new Error('missing classifier outputs');
if ((required === 'true' || releaseOnly === 'true') && eventName !== 'pull_request') throw new Error('non-PR classifier outputs are inconsistent');
if (eventName === 'pull_request' && required === releaseOnly) throw new Error('PR classifier outputs are inconsistent');
if (required === 'true' || (eventName === 'workflow_dispatch' && manual === 'true')) {
  if (needs['check-macos']?.result !== 'success') throw new Error('required Mac proof failed, cancelled, skipped or missing');
  console.log('Mac proof passed');
} else {
  if (eventName === 'pull_request' && releaseOnly !== 'true') throw new Error('PR omission was not authorized');
  if (needs['check-macos']?.result !== 'skipped') throw new Error('omitted Mac lane must be skipped');
  console.log('Mac proof intentionally omitted by policy; no Mac tests ran');
}
