const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const read = name => JSON.parse(fs.readFileSync(`src-tauri/${name}`, 'utf8'));
test('shared and Linux configuration do not inherit Windows resources or installer settings', () => {
  const base = read('tauri.conf.json');
  const linux = read('tauri.linux.conf.json');
  for (const config of [base, linux]) {
    assert.equal(config.bundle.windows, undefined);
    assert.equal(config.bundle.resources, undefined);
    assert.ok(!JSON.stringify(config).includes('.exe'));
  }
  assert.deepEqual(linux.bundle.targets, ['deb', 'appimage']);
});
test('Windows overlay retains both executables and exact NSIS installation behavior', () => {
  const config = read('tauri.windows.conf.json');
  assert.deepEqual(config.bundle.targets, ['nsis']);
  assert.deepEqual(config.bundle.resources, ['resources/minerdesk-headless.exe', 'resources/minerdesk-backend.exe']);
  assert.deepEqual(config.bundle.windows.nsis, {
    installMode: 'perMachine', displayLanguageSelector: true,
    languages: ['English', 'French'], installerHooks: './windows/hooks.nsh'
  });
});
