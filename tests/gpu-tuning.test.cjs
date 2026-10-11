const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),Module=require('node:module');
const ts=require('typescript'),React=require('react');
const {create,act}=require('react-test-renderer');
function production(name) {
  const filename=path.resolve(__dirname,'../src/'+name),mod=new Module(filename,module);
  mod.paths=Module._nodeModulePaths(path.dirname(filename));const original=mod.require.bind(mod);
  mod.require=id=>id.startsWith('./') ? production(id.slice(2)+'.ts') : original(id);
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText,filename);
  return mod.exports;
}
const {effectiveGpuTuning,changeGpuTuning,clearGpuTuning,invalidSrbPower,invalidGpuTuning,supportedGpuTuning}=production('gpuTuning.ts');
const Panel=production('GpuTuningPanel.tsx').default;
const profile=()=>({id:'fixture',engine:'srbminer',gpu_ids:'0',gpu_tuning:[],core_clock:2100,power_limit:2100,fan:60,wallet:'unchanged-wallet'});
test('older rows retain inheritance and explicit clearing survives serialization',()=>{
  const p=profile();p.gpu_tuning=[{selector:'0',core_clock:2200,power_limit:null,fan:null},{selector:'1',core_clock:2300,power_limit:160,fan:50}];
  assert.equal(effectiveGpuTuning(p,'0').power_limit,2100);
  const next={...p,gpu_tuning:changeGpuTuning(p,'0',{power_limit:null})};
  const restored=JSON.parse(JSON.stringify(next));
  assert.equal(effectiveGpuTuning(restored,'0').power_limit,null);
  assert.equal(effectiveGpuTuning(restored,'0').core_clock,2200);
  assert.equal(effectiveGpuTuning(restored,'0').fan,60);
  assert.deepEqual(restored.gpu_tuning.find(row=>row.selector==='1'),p.gpu_tuning[1]);
  assert.equal(restored.power_limit,2100);assert.equal(invalidSrbPower(restored),null);
});

test('memory clocks and signed offsets survive edits, clearing and serialization',()=>{
  const p={...profile(),power_limit:180,memory_clock:810,core_offset:-100,memory_offset:500};
  assert.equal(effectiveGpuTuning(p,'0').memory_clock,810);
  assert.equal(effectiveGpuTuning(p,'0').core_offset,-100);
  const next=JSON.parse(JSON.stringify({...p,gpu_tuning:changeGpuTuning(p,'0',{memory_clock:null,memory_offset:-200})}));
  const effective=effectiveGpuTuning(next,'0');
  assert.equal(effective.memory_clock,null);assert.equal(effective.memory_offset,-200);assert.equal(effective.core_offset,-100);assert.equal(effective.core_clock,2100);
  assert.equal(invalidGpuTuning(next),null);
  next.gpu_tuning[0].core_offset=-100.5;assert.equal(invalidGpuTuning(next),'core_offset');
  next.gpu_tuning[0].core_offset=-100;next.gpu_tuning[0].memory_clock=-1;assert.equal(invalidGpuTuning(next),'memory_clock');
  const cleared={...next,...clearGpuTuning()};for(const field of ['memory_clock','core_offset','memory_offset'])assert.equal(cleared[field],null);
  assert.equal(cleared.wallet,p.wallet);assert.equal(cleared.gpu_ids,p.gpu_ids);
});

test('engine capabilities and interactive memory controls show only implemented options',async()=>{
  assert.equal(supportedGpuTuning('srbminer').length,6);
  assert.deepEqual(supportedGpuTuning('npminer'),['core_clock','memory_clock','power_limit']);
  assert.deepEqual(supportedGpuTuning('lpminer'),['core_clock']);assert.deepEqual(supportedGpuTuning('custom'),[]);
  let current={...profile(),power_limit:180,memory_clock:null,core_offset:null,memory_offset:null},view;
  function Fixture(){const[p,set]=React.useState(current);current=p;return React.createElement(Panel,{profile:p,devices:[{selector:'0',name:'Fixture NVIDIA'}],language:'fr',onGpuChange:(id,patch)=>set(old=>({...old,gpu_tuning:changeGpuTuning(old,id,patch)})),onProfileChange:patch=>set(old=>({...old,...patch}))});}
  await act(async()=>{view=create(React.createElement(Fixture));});
  const input=label=>view.root.findAllByType('input').find(i=>i.props['aria-label']===label+' · GPU 0');
  const memory=()=>input('Fréquence mémoire GPU MHz'),offset=()=>input('Décalage mémoire MHz (+/−)');
  await act(async()=>memory().props.onChange({target:{value:'810'}}));
  await act(async()=>offset().props.onChange({target:{value:'-200'}}));
  assert.equal(memory().props.value,810);assert.equal(offset().props.value,-200);assert.equal(offset().props.min,undefined);
  await act(async()=>memory().props.onChange({target:{value:''}}));assert.equal(effectiveGpuTuning(current,'0').memory_clock,null);assert.equal(offset().props.value,-200);
  await act(async()=>view.update(React.createElement(Panel,{profile:{...current,engine:'npminer'},devices:[{selector:'0',name:'Fixture NVIDIA'}],language:'en',onGpuChange:()=>{},onProfileChange:()=>{}})));
  assert(view.root.findAllByType('input').every(i=>!i.props['aria-label'].includes('offset')&&!i.props['aria-label'].includes('Fan')));
  await act(async()=>view.unmount());
});
test('clear all removes per-GPU and legacy tuning without changing selection or wallet',()=>{
  const p=profile();p.gpu_tuning=[{selector:'0',core_clock:2200,power_limit:150,fan:50}];
  const next={...p,...clearGpuTuning()};
  assert.deepEqual(next.gpu_tuning,[]);assert.equal(next.power_limit,null);assert.equal(next.core_clock,null);assert.equal(next.fan,null);
  assert.equal(next.gpu_ids,p.gpu_ids);assert.equal(next.wallet,p.wallet);
});
test('power validation distinguishes watts from GPU MHz and only checks selected settings',()=>{
  const p=profile();assert.equal(invalidSrbPower(p),2100);
  p.power_limit=180;assert.equal(invalidSrbPower(p),null);
  p.core_clock=2745;assert.equal(invalidSrbPower(p),null);
  p.power_limit=1.5;assert.equal(invalidSrbPower(p),1.5);
  p.engine='lpminer';assert.equal(invalidSrbPower(p),null);
});
test('effective defaults are visible, clearing a GPU input does not revive them, and reset works in French',async()=>{
  let current=profile(),view;
  function Fixture() {
    const [p,set]=React.useState(current);current=p;
    return React.createElement(Panel,{profile:p,devices:[{selector:'0',name:'Fixture NVIDIA'}],language:'fr',
      onGpuChange:(id,patch)=>set(old=>({...old,gpu_tuning:changeGpuTuning(old,id,patch)})),
      onProfileChange:patch=>set(old=>({...old,...patch}))});
  }
  await act(async()=>{view=create(React.createElement(Fixture));});
  const power=()=>view.root.findAllByType('input').find(input=>input.props['aria-label'].includes('GPU 0') && input.props.max===1000);
  assert.equal(power().props.value,2100);assert.equal(view.root.findByType('details').props.open,true);
  assert(view.root.findByProps({role:'alert'}));
  await act(async()=>power().props.onChange({target:{value:''}}));
  assert.equal(power().props.value,'');assert.equal(effectiveGpuTuning(current,'0').power_limit,null);
  assert.equal(view.root.findAllByProps({role:'alert'}).length,0);
  await act(async()=>view.root.findByType('button').props.onClick());
  assert.equal(current.power_limit,null);assert.equal(current.core_clock,null);assert.equal(current.wallet,'unchanged-wallet');
  await act(async()=>view.unmount());
});
