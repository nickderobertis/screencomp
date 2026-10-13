// GitHub job outcomes are the only doubled boundary in the subprocess tests.
const needs = JSON.parse(process.env.NEEDS_JSON ?? 'null');
if (!needs || needs.policy?.result !== 'success') throw new Error('Mac policy prerequisite failed, cancelled or missing');
const required = needs.policy.outputs?.macosRequired;
const releaseOnly = needs.policy.outputs?.isReleaseOnly;
if (!['true', 'false'].includes(required) || !['true', 'false'].includes(releaseOnly)) throw new Error('missing classifier outputs');
if (required === 'true' || (process.env.GITHUB_EVENT_NAME === 'workflow_dispatch' && process.env.MANUAL_MACOS === 'true')) {
  if (needs['check-macos']?.result !== 'success') throw new Error('required Mac proof failed, cancelled, skipped or missing');
  console.log('Mac proof passed');
} else {
  if (process.env.GITHUB_EVENT_NAME === 'pull_request' && releaseOnly !== 'true') throw new Error('PR omission was not authorized');
  if (needs['check-macos']?.result !== 'skipped') throw new Error('omitted Mac lane must be skipped');
  console.log('Mac proof intentionally omitted by policy; no Mac tests ran');
}
