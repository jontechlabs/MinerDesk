const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),Module=require('node:module'),ts=require('typescript'),React=require('react');
const {create,act}=require('react-test-renderer');
function production(name){
  const filename=path.resolve(__dirname,'../src',name),mod=new Module(filename,module);
  mod.paths=Module._nodeModulePaths(path.dirname(filename));const original=mod.require.bind(mod);
  mod.require=id=>id.startsWith('./')?production(id.slice(2)+'.ts'):original(id);
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText,filename);
  return mod.exports;
}
const {default:Controls,ScheduleBadge}=production('MinerScheduleControls.tsx');
const text=node=>Array.isArray(node)?node.map(text).join(''):typeof node==='string'?node:(node?.children||[]).map(text).join('');
const fixture={profile:{id:'gpu',enabled:true},runtime:{running:false,started_by:'',last_error:''},scheduled_now:true,scheduler_paused:false};
function render(overrides={},language='en',hasSchedule=true){
  const actions=[],status={...fixture,...overrides,runtime:{...fixture.runtime,...overrides.runtime}},props={status,language,hasSchedule,busy:false,connected:true,onAction:a=>actions.push(a),onConfig:()=>{}};
  let view;act(()=>view=create(React.createElement(React.Fragment,null,React.createElement(Controls,props),React.createElement(ScheduleBadge,props))));
  return {view,actions,content:()=>text(view.toJSON()),button:label=>view.root.findAllByType('button').find(b=>text(b.toJSON?b.toJSON():{children:b.children})===label),close:()=>act(()=>view.unmount())};
}
test('ordinary Start and explicit Force mining send different commands',()=>{
  const r=render();act(()=>r.button('Start').props.onClick());act(()=>r.button('Force mining').props.onClick());
  assert.deepEqual(r.actions,['start','force-start']);r.close();
});
test('a running forced session is clearly labeled and can return to its schedule',()=>{
  const r=render({runtime:{running:true,started_by:'manual'}});
  assert.match(r.content(),/forced · no time limit/);assert(!r.content().includes('⏱ scheduled'));
  act(()=>r.button('Resume schedule').props.onClick());assert.deepEqual(r.actions,['start']);r.close();
});
test('outside a window the UI explains waiting instead of promising immediate mining',()=>{
  const r=render({scheduled_now:false});assert.match(r.content(),/waiting for scheduled window/);
  act(()=>r.button('Resume schedule').props.onClick());assert.deepEqual(r.actions,['start']);r.close();
});
test('scheduled running sessions expose Stop and a schedule-aware Restart',()=>{
  const r=render({runtime:{running:true,started_by:'schedule'}});assert.match(r.content(),/⏱ scheduled/);
  act(()=>r.button('Restart').props.onClick());act(()=>r.button('Stop').props.onClick());assert.deepEqual(r.actions,['restart','stop']);r.close();
});
test('paused and disabled profiles do not appear to be waiting or actively scheduled',()=>{
  const r=render({scheduler_paused:true});assert.match(r.content(),/schedule paused manually/);assert(!r.content().includes('waiting for scheduled window'));r.close();
  const disabled=render({profile:{id:'gpu',enabled:false},scheduled_now:false});
  assert(!disabled.content().includes('waiting for scheduled window'));assert(disabled.button('Resume schedule').props.disabled);assert(disabled.button('Force mining').props.disabled);disabled.close();
});
test('French labels distinguish forced mining, waiting and schedule resumption',()=>{
  const r=render({runtime:{running:true,started_by:'manual'}},'fr');assert.match(r.content(),/forcé · sans limite horaire/);assert(r.button('Reprendre le planning'));r.close();
});
