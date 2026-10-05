const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const Module = require('node:module');
const ts = require('typescript');
const filename = require('node:path').resolve('src/devTip.ts');
const mod = new Module(filename, module);
mod._compile(ts.transpileModule(fs.readFileSync(filename, 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
}).outputText, filename);
const { detectDevCoin } = mod.exports;
const profile = changes => ({ wallet: '', algorithm: '', pool: '', ...changes });

test('Quantus recognition handles algorithm and pool conventions', () => {
  for (const p of [profile({ algorithm: ' QUANTUS ' }), profile({ pool: 'stratum+tcp://QTC.kryptex.network:7049' }), profile({ pool: 'eu.quantus.example:1234' })]) {
    assert.equal(detectDevCoin(p), 'QTC');
  }
});
test('Quantus public address is recognized with either worker separator', () => {
  const rust = fs.readFileSync('src-tauri/src/lib.rs', 'utf8');
  const address = rust.match(/const DEV_WALLET_QTC: &str = "([^"]+)"/)[1];
  for (const suffix of ['', '/QuantusRig', '.QuantusRig']) assert.equal(detectDevCoin(profile({ wallet: address + suffix })), 'QTC');
});
test('Poseidon2 alone and unrelated pools or malformed addresses remain unsupported', () => {
  for (const p of [profile({ algorithm: 'poseidon2' }), profile({ pool: 'notqtc.example:1234' }), profile({ wallet: 'qz-not-an-address' })]) {
    assert.equal(detectDevCoin(p), null);
  }
});
test('Existing Pearl and EVM recognition is preserved', () => {
  assert.equal(detectDevCoin(profile({ wallet: 'prl1example', algorithm: 'pearlhash' })), 'PRL');
  assert.equal(detectDevCoin(profile({ wallet: '0x1234' })), 'EVM');
});
