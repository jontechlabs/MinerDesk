const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),Module=require('node:module'),ts=require('typescript'),React=require('react');
const {create,act}=require('react-test-renderer');
function production(name){
  const filename=path.resolve(__dirname,'../src',name),mod=new Module(filename,module);
  mod.paths=Module._nodeModulePaths(path.dirname(filename));const original=mod.require.bind(mod);
  mod.require=id=>id.startsWith('./')?production(id.slice(2)+'.ts'):original(id);
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText,filename);
  return mod.exports;
}
const Panel=production('GpuDiagnostic.tsx').default;
const text=node=>Array.isArray(node)?node.map(text).join(''):typeof node==='string'?node:(node?.children||[]).map(text).join('');
const discovery={engine:'srbminer',platform:'windows',source:'system',selectors_verified:false,diagnostic_code:'windows_access_denied',os_error:5,error_detail:'Accès refusé. (os error 5)',devices:[],raw_excerpt:'',selection_hint:''};
function render(overrides={},language='en'){
  let view;act(()=>{view=create(React.createElement(Panel,{discovery:{...discovery,...overrides},language}))});
  const content=text(view.toJSON());act(()=>view.unmount());return content;
}

test('Windows access denial shows its exact native error and protection-history guidance',()=>{
  const content=render();assert.match(content,/Windows refused access/);assert.match(content,/protection history/);
  assert.match(content,/Accès refusé\. \(os error 5\)/);assert.match(content,/does not identify the cause/);
  assert(!content.includes('Linux'));assert(!content.includes('chmod'));assert(!content.includes('noexec'));
});
test('launch advice follows the backend platform for remote browsers',()=>{
  assert.match(render({diagnostic_code:'launch_failed',platform:'linux'}),/noexec/);
  const windows=render({diagnostic_code:'launch_failed'});assert.match(windows,/Windows could not launch/);assert(!windows.includes('noexec'));
  const older=render({diagnostic_code:'launch_failed',platform:undefined,error_detail:undefined});assert.match(older,/Check the exact error/);assert(!older.includes('Linux'));
});
test('French access refusal preserves the exact error without guessing antivirus involvement',()=>{
  const content=render({},'fr');assert.match(content,/Windows refuse l’accès/);assert.match(content,/historique de protection/);
  assert.match(content,/ne permet pas à elle seule/);assert.match(content,/Accès refusé/);assert(!content.includes('Linux'));
});
test('a successful listing has no failure panel',()=>{
  assert.equal(render({diagnostic_code:null,error_detail:null}), '');
});
