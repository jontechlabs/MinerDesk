const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const filename = path.resolve(__dirname, '../src/gpuDiscovery.ts');
const production = new Module(filename, module);
production._compile(ts.transpileModule(fs.readFileSync(filename, 'utf8'), {
  compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022},
}).outputText, filename);
const {selectableGpuDevices, gpuSelectorsUnverified} = production.exports;
const system = {engine:'srbminer', source:'system', selectors_verified:false, raw_excerpt:'', selection_hint:'',
  devices:[{selector:'0',name:'NVIDIA GeForce RTX 5060 Ti',vendor:'NVIDIA',pci_bus:'0000:01:00.0'}]};

test('system CUDA index 0 cannot select or tune SRBMiner GPU 0', () => {
  assert.deepEqual(selectableGpuDevices(system,'srbminer'),[]);
  assert.equal(gpuSelectorsUnverified(system,'srbminer'),true);
  assert.equal(system.devices[0].selector,'0');
});
test('verified mixed engine listing keeps the RTX on global GPU 1', () => {
  const discovery = {...system, source:'srbminer --list-devices', selectors_verified:true,
    devices:[{selector:'0',name:'AMD Radeon Graphics'}, {...system.devices[0],selector:'1'}]};
  assert.deepEqual(selectableGpuDevices(discovery,'srbminer').map(g=>g.selector),['0','1']);
  assert.equal(gpuSelectorsUnverified(discovery,'srbminer'),false);
});
test('fallback selectors from other indexed engines and older responses are unverified', () => {
  for(const engine of ['lolminer','rigel','bzminer','npminer']) {
    assert.deepEqual(selectableGpuDevices({...system,engine},engine),[]);
  }
  const {selectors_verified,...olderBackend} = system;
  assert.deepEqual(selectableGpuDevices(olderBackend,'srbminer'),[]);
});
test('a different engine discovery cannot affect the current profile', () => {
  assert.deepEqual(selectableGpuDevices({...system,selectors_verified:true},'rigel'),[]);
  assert.deepEqual(selectableGpuDevices(null,'srbminer'),[]);
  assert.equal(gpuSelectorsUnverified(null,'srbminer'),false);
});
test('lpminer system inventory remains usable for its existing shared-clock workflow', () => {
  const discovery = {...system,engine:'lpminer'};
  assert.equal(selectableGpuDevices(discovery,'lpminer'),discovery.devices);
  assert.equal(gpuSelectorsUnverified(discovery,'lpminer'),false);
});
