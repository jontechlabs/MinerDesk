const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { prepareRequest, runAction, readActionOutput } = require('../scripts/signpath-sign.cjs');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'md-signing-test-'));
const sample = path.join(temp, 'MinerDesk.exe');
fs.writeFileSync(sample, 'fixture');
const env = {
  GITHUB_ACTIONS: 'true', GITHUB_REPOSITORY: 'jontechlabs/MinerDesk', GITHUB_REF: 'refs/heads/main', GITHUB_EVENT_NAME: 'workflow_dispatch',
  ACTIONS_RUNTIME_TOKEN: 'synthetic', ACTIONS_RESULTS_URL: 'synthetic', MD_SIGNING_TOOLS: temp,
  SIGNPATH_API_TOKEN: 'synthetic', SIGNPATH_ORGANIZATION_ID: 'synthetic', SIGNPATH_PROJECT_SLUG: 'MinerDesk',
  SIGNPATH_POLICY_SLUG: 'release-signing', MD_SIGNING_PUBLISHER: 'SignPath Foundation', MD_SIGNING_VERSION: '0.7.23', GITHUB_TOKEN: 'synthetic'
};
test.after(() => fs.rmSync(temp, { recursive: true, force: true }));
test('only upstream main can submit signing artifacts', () => {
  assert.equal(prepareRequest([sample], env)[0].file, sample);
  for (const patch of [{ GITHUB_ACTIONS: '' }, { GITHUB_REPOSITORY: 'attacker/MinerDesk' },
    { GITHUB_REF: 'refs/pull/1/merge' }, { GITHUB_REF: 'refs/heads/feature' }, { GITHUB_EVENT_NAME: 'pull_request_target' }]) {
    assert.throws(() => prepareRequest([sample], { ...env, ...patch }), /restricted/);
  }
});
test('missing provider credentials or artifact runtime fails before upload', () => {
  for (const key of Object.keys(env).filter(key => !key.startsWith('GITHUB_') || key === 'GITHUB_TOKEN')) {
    assert.throws(() => prepareRequest([sample], { ...env, [key]: '' }), /Missing signing configuration/);
  }
});
test('explicit existing files only; secrets and duplicate basenames cannot be uploaded', () => {
  assert.throws(() => prepareRequest([], env), /No files/);
  assert.throws(() => prepareRequest([sample, sample], env), /duplicate/);
  const secret = path.join(temp, 'certificate.pfx');
  fs.writeFileSync(secret, 'fixture');
  assert.throws(() => prepareRequest([secret], env), /Unsupported/);
  assert.throws(() => prepareRequest([temp], env), /absolute file paths/);
});
test('provider action failure stops signing', () => {
  const failure = path.join(temp, 'failure.cjs');
  fs.writeFileSync(failure, 'process.exit(7);');
  assert.throws(() => runAction(failure, {}, env, path.join(temp, 'outputs')), /no unsigned fallback/);
});
test('NSIS temporary PE gets an EXE artifact name while retaining its destination path', () => {
  const uninstaller = path.join(temp, 'nst3399.tmp');
  fs.writeFileSync(uninstaller, 'MZsynthetic-test-PE');
  assert.deepEqual(prepareRequest([uninstaller], env), [{ file: uninstaller, name: 'MinerDesk-uninstaller.exe' }]);
  fs.writeFileSync(uninstaller, 'recovery text is not an executable');
  assert.throws(() => prepareRequest([uninstaller], env), /not a PE executable/);
  const arbitrary = path.join(temp, 'private.tmp');
  fs.writeFileSync(arbitrary, 'MZ');
  assert.throws(() => prepareRequest([arbitrary], env), /Unsupported/);
});
test('action inputs and output channel remain scoped to the child process', () => {
  const check = path.join(temp, 'check.cjs');
  fs.writeFileSync(check, `const fs=require('node:fs'); if(!fs.existsSync(process.env.GITHUB_OUTPUT))process.exit(4); if(process.env.INPUT_UNRELATED)process.exit(2);
    if(process.env.INPUT_NAME!=='fixture')process.exit(3); fs.writeFileSync(process.env.GITHUB_OUTPUT,'artifact-id=123\\n');`);
  const output = path.join(temp, 'outputs');
  runAction(check, { name: 'fixture' }, { ...env, INPUT_UNRELATED: 'do-not-inherit' }, output);
  assert.equal(fs.readFileSync(output, 'utf8'), 'artifact-id=123\n');
  assert.equal(process.env.INPUT_NAME, undefined);
});
test('artifact IDs support the official multiline GitHub file-command format', () => {
  const output = 'artifact-url<<url_delimiter\r\nhttps://example.test\r\nurl_delimiter\r\nartifact-id<<ghadelimiter_test\r\n123\r\nghadelimiter_test\r\n';
  assert.equal(readActionOutput(output, 'artifact-id'), '123');
  assert.equal(readActionOutput('artifact-id=456\n', 'artifact-id'), '456');
  assert.equal(readActionOutput(output, 'absent'), undefined);
  assert.throws(() => readActionOutput('artifact-id<<unterminated\n123', 'artifact-id'), /Incomplete/);
});
