'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { runAction, readActionOutput } = require('../../../scripts/signpath-sign.cjs');
try {
  if (!process.env.ACTIONS_RUNTIME_TOKEN || !process.env.ACTIONS_RESULTS_URL) throw new Error('GitHub artifact runtime is unavailable.');
  const temp = process.env.RUNNER_TEMP;
  if (!temp || !path.isAbsolute(temp)) throw new Error('Invalid runner temp directory.');
  const fixture = path.join(temp, `md-signing-runtime-${crypto.randomUUID()}.txt`);
  const output = path.join(temp, `md-signing-outputs-${crypto.randomUUID()}.txt`);
  try {
    fs.writeFileSync(fixture, 'MinerDesk signing uploader contract test. No executable or secret.\n');
    runAction(path.join(process.env.MD_UPLOAD_ACTION, 'dist', 'upload', 'index.js'), {
      name: 'signing-uploader-test', path: fixture, 'if-no-files-found': 'error', 'retention-days': 1,
      'compression-level': 0, overwrite: false, 'include-hidden-files': false
    }, process.env, output);
    const id = readActionOutput(fs.readFileSync(output, 'utf8'), 'artifact-id');
    if (!id || !/^\d+$/.test(id)) throw new Error('No GitHub artifact ID returned.');
    console.log('PASS: the actual official uploader works inside the signing callback runtime.');
  } finally {
    for (const file of [fixture, output]) if (fs.existsSync(file)) fs.unlinkSync(file);
  }
} catch (error) { console.error(error.message); process.exitCode = 1; }
