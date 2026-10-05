'use strict';
// Called from a JavaScript GitHub action so the official upload-artifact client
// inherits GitHub's artifact runtime credentials. No credentials are persisted.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

function required(env, name) {
  if (!env[name]?.trim()) throw new Error(`Missing signing configuration: ${name}`);
  return env[name];
}
function prepareRequest(files, env) {
  if (env.GITHUB_ACTIONS !== 'true' || env.GITHUB_REPOSITORY !== 'jontechlabs/MinerDesk' || env.GITHUB_REF !== 'refs/heads/main' || env.GITHUB_EVENT_NAME !== 'workflow_dispatch') {
    throw new Error('Signing is restricted to the upstream main branch on GitHub Actions.');
  }
  for (const key of ['ACTIONS_RUNTIME_TOKEN', 'ACTIONS_RESULTS_URL', 'MD_SIGNING_TOOLS', 'SIGNPATH_API_TOKEN',
    'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG', 'SIGNPATH_POLICY_SLUG', 'MD_SIGNING_PUBLISHER',
    'MD_SIGNING_VERSION', 'GITHUB_TOKEN']) required(env, key);
  if (!Array.isArray(files) || !files.length) throw new Error('No files requested for signing.');
  const names = new Set();
  return files.map(file => {
    if (!path.isAbsolute(file) || !fs.statSync(file).isFile()) throw new Error('Signing requires existing absolute file paths.');
    let name = path.basename(file);
    // NSIS !uninstfinalize supplies nstXXXX.tmp, which is a PE executable.
    // Give that one file an EXE name inside the submitted artifact, then copy
    // the signed bytes back to the exact temporary path expected by NSIS.
    if (/^nst[0-9a-f]+\.tmp$/i.test(name)) {
      const handle = fs.openSync(file, 'r');
      const header = Buffer.alloc(2);
      try { fs.readSync(handle, header, 0, 2, 0); } finally { fs.closeSync(handle); }
      if (header.toString('ascii') !== 'MZ') throw new Error('Temporary NSIS file is not a PE executable.');
      name = 'MinerDesk-uninstaller.exe';
    }
    if (!/\.(exe|ps1)$/i.test(name) || names.has(name.toLowerCase())) throw new Error('Unsupported or duplicate signing filename.');
    names.add(name.toLowerCase());
    return { file, name };
  });
}
function runAction(script, inputs, env, outputFile) {
  // Do not inherit INPUT_* from our containing action into another action.
  const childEnv = Object.fromEntries(Object.entries(env).filter(([key]) => !key.startsWith('INPUT_')));
  for (const [key, value] of Object.entries(inputs)) childEnv[`INPUT_${key.toUpperCase()}`] = String(value);
  childEnv.GITHUB_OUTPUT = outputFile;
  // @actions/core requires this file to exist before setOutput appends to it.
  fs.writeFileSync(outputFile, '');
  const result = spawnSync(process.execPath, [script], { env: childEnv, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error('Official signing/upload action failed; no unsigned fallback.');
}
function readActionOutput(contents, name) {
  const lines = contents.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith(`${name}=`)) return lines[i].slice(name.length + 1);
    if (lines[i].startsWith(`${name}<<`)) {
      const delimiter = lines[i].slice(name.length + 2);
      const value = [];
      while (++i < lines.length && lines[i] !== delimiter) value.push(lines[i]);
      if (i === lines.length) throw new Error('Incomplete GitHub action output record.');
      return value.join('\n');
    }
  }
  return undefined;
}
function main(requestFile) {
  const files = prepareRequest(JSON.parse(fs.readFileSync(requestFile, 'utf8').replace(/^\uFEFF/, '')), process.env);
  const runnerTemp = required(process.env, 'RUNNER_TEMP');
  if (!path.isAbsolute(runnerTemp)) throw new Error('RUNNER_TEMP must be an absolute path.');
  const temp = fs.mkdtempSync(path.join(runnerTemp, 'md-signpath-'));
  try {
    const inputDir = path.join(temp, 'unsigned');
    const outputDir = path.join(temp, 'signed');
    fs.mkdirSync(inputDir);
    for (const { file, name } of files) fs.copyFileSync(file, path.join(inputDir, name));
    const uploadOutput = path.join(temp, 'upload-output.txt');
    runAction(path.join(process.env.MD_SIGNING_TOOLS, 'upload-artifact', 'dist', 'upload', 'index.js'), {
      name: `signing-${crypto.randomUUID()}`, path: inputDir, 'if-no-files-found': 'error', 'retention-days': 1,
      'compression-level': 0, overwrite: false, 'include-hidden-files': false
    }, process.env, uploadOutput);
    const id = readActionOutput(fs.readFileSync(uploadOutput, 'utf8'), 'artifact-id');
    if (!id || !/^\d+$/.test(id)) throw new Error('GitHub did not return an artifact ID.');
    runAction(path.join(process.env.MD_SIGNING_TOOLS, 'signpath', 'index.js'), {
      'connector-url': 'https://pipelineconnector.connectors.signpath.io/GitHubActions/GitHubCom',
      'api-token': process.env.SIGNPATH_API_TOKEN, 'github-token': process.env.GITHUB_TOKEN,
      'organization-id': process.env.SIGNPATH_ORGANIZATION_ID,
      'project-slug': process.env.SIGNPATH_PROJECT_SLUG, 'signing-policy-slug': process.env.SIGNPATH_POLICY_SLUG,
      'artifact-configuration-slug': 'minerdesk-windows', 'github-artifact-id': id,
      'wait-for-completion': true, 'wait-for-completion-timeout-in-seconds': 1800,
      'service-unavailable-timeout-in-seconds': 120, 'download-signed-artifact-timeout-in-seconds': 300,
      'output-artifact-directory': outputDir, 'skip-decompress': false,
      parameters: `version: ${JSON.stringify(process.env.MD_SIGNING_VERSION)}`
    }, process.env, path.join(temp, 'sign-output.txt'));
    const returned = fs.readdirSync(outputDir).sort();
    if (JSON.stringify(returned) !== JSON.stringify(files.map(({ name }) => name).sort())) {
      throw new Error('Signed artifact does not contain exactly the requested files.');
    }
    // Validate every returned signature BEFORE replacing any input file.
    for (const { name } of files) {
      const check = spawnSync('powershell.exe', ['-NoProfile', '-File', path.join(__dirname, 'verify-windows-signatures.ps1'),
        '-Path', path.join(outputDir, name), '-ExpectedPublisher', process.env.MD_SIGNING_PUBLISHER], { stdio: 'inherit' });
      if (check.error || check.status !== 0) throw new Error('Returned file failed Authenticode verification.');
    }
    for (const { file, name } of files) fs.copyFileSync(path.join(outputDir, name), file);
  } finally {
    if (path.dirname(path.resolve(temp)) !== path.resolve(runnerTemp) || !path.basename(temp).startsWith('md-signpath-')) {
      throw new Error('Refusing to remove a directory outside the signing workspace.');
    }
    fs.rmSync(temp, { recursive: true, force: true });
  }
}
module.exports = { prepareRequest, runAction, readActionOutput };
if (require.main === module) {
  try { main(process.argv[2]); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
