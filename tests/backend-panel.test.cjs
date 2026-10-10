const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),Module=require('node:module'),ts=require('typescript'),React=require('react');
const {create,act}=require('react-test-renderer');
function production(name){
  const filename=path.resolve(__dirname,'../src',name),mod=new Module(filename,module);
  mod.paths=Module._nodeModulePaths(path.dirname(filename));const original=mod.require.bind(mod);
  mod.require=id=>id.startsWith('./')?production(id.slice(2)+'.ts'):original(id);
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText,filename);
  return mod.exports;
}
const Panel=production('BackendPanel.tsx').default;
const text=node=>Array.isArray(node)?node.map(text).join(''):typeof node==='string'?node:(node?.children||[]).map(text).join('');
const status={supported:false,reachable:false,port:17888,task_installed:false,task_state:'Not applicable',process_running:false,listener_pid:null,listener_process:'',executable_path:'/usr/bin/minerdesk',last_task_result:'',log_tail:'Desktop backend 127.0.0.1:17888: Address already in use'};
function render(extra={},language='en'){let view,calls=[];act(()=>{view=create(React.createElement(Panel,{status:{...status,...extra},language,busy:false,onAction:c=>calls.push(c),onRefresh:()=>calls.push('refresh')}))});return {view,calls};}
function click(view,label){act(()=>view.root.findAllByType('button').find(b=>text(b)===label).props.onClick());}
test('offline Linux shows the built-in server error and permits Start without Windows tasks',()=>{
  const {view,calls}=render();const content=text(view.toJSON());
  assert.match(content,/Desktop backend/);assert.match(content,/Address already in use/);
  for(const wrong of ['Privileged backend','task missing','Scheduled task','Repair backend','Restart backend'])assert(!content.includes(wrong));
  click(view,'Start backend');click(view,'Refresh');assert.deepEqual(calls,['start_privileged_backend','refresh']);act(()=>view.unmount());
});
test('healthy Linux does not expose unsupported repair or restart commands',()=>{
  const {view}=render({reachable:true});assert.match(text(view.toJSON()),/Connected · 127.0.0.1:17888/);
  assert.deepEqual(view.root.findAllByType('button').map(text),['Refresh']);act(()=>view.unmount());
});
test('Windows keeps task diagnostics, repair and restart',()=>{
  let {view,calls}=render({supported:true,task_state:'Missing'});assert.match(text(view.toJSON()),/task missing/);assert.match(text(view.toJSON()),/Scheduled task/);
  click(view,'Repair backend');assert.deepEqual(calls,['repair_privileged_backend']);act(()=>view.unmount());
  ({view,calls}=render({supported:true,reachable:true}));click(view,'Restart backend');assert.deepEqual(calls,['restart_privileged_backend']);act(()=>view.unmount());
});
test('French Linux diagnostics use platform-specific translations and busy controls',()=>{
  const {view}=render({},'fr');assert.match(text(view.toJSON()),/serveur intégré indisponible/);assert(!text(view.toJSON()).includes('tâche absente'));act(()=>view.unmount());
  let busy;act(()=>busy=create(React.createElement(Panel,{status,language:'en',busy:true,onAction:()=>{},onRefresh:()=>{}})));
  assert(busy.root.findAllByType('button').every(b=>b.props.disabled));assert.match(text(busy.toJSON()),/Starting…/);act(()=>busy.unmount());
});
