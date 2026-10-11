const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),Module=require('node:module');
const ts=require('typescript'),React=require('react'),{create,act}=require('react-test-renderer');
function production(name){const filename=path.resolve(__dirname,'../src/'+name),mod=new Module(filename,module);mod.paths=Module._nodeModulePaths(path.dirname(filename));mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText,filename);return mod.exports;}
const Control=production('LinuxElevationControl.tsx').default;
const {runMiningCommand}=production('minerCommands.ts');
test('root setting uses backend platform, persists explicit edits and can be disabled when helper is missing',async()=>{
 let view,current={id:'fixture'};
 function Fixture(){const[p,set]=React.useState(current);current=p;return React.createElement(Control,{profile:p,platform:'linux',available:true,language:'fr',onChange:patch=>set(old=>({...old,...patch}))});}
 await act(async()=>{view=create(React.createElement(Fixture));});
 assert.equal(view.root.findByType('input').props.checked,false);
 await act(async()=>view.root.findByType('input').props.onChange({target:{checked:true}}));
 assert.equal(JSON.parse(JSON.stringify(current)).linux_run_as_root,true);
 await act(async()=>view.update(React.createElement(Control,{profile:current,platform:'linux',available:false,language:'en',onChange:()=>{}})));
 assert.equal(view.root.findByType('input').props.disabled,false);
 await act(async()=>view.update(React.createElement(Control,{profile:{id:'fixture'},platform:'linux',available:false,language:'en',onChange:()=>{}})));
 assert.equal(view.root.findByType('input').props.disabled,true);
 await act(async()=>view.update(React.createElement(Control,{profile:current,platform:'windows',available:true,language:'en',onChange:()=>{}})));
 assert.equal(view.toJSON(),null);await act(async()=>view.unmount());
});
test('only elevated starts allow time for each system approval; stop never waits for approval',async()=>{
 const calls=[],base={id:'fixture',dirty:false,token:'',save:async()=>{},request:async(p,o,t,timeout)=>{calls.push({p,timeout});return p.endsWith('start-all')?[{id:'fixture',result:null}]:{ok:true};}};
 await runMiningCommand({...base,action:'start',elevatedStarts:1});
 await runMiningCommand({...base,action:'restart',elevatedStarts:1});
 await runMiningCommand({...base,action:'start-all',elevatedStarts:2});
 await runMiningCommand({...base,action:'stop',elevatedStarts:1});
 await runMiningCommand({...base,action:'start'});
 assert.deepEqual(calls.map(c=>c.timeout),[160000,160000,290000,30000,30000]);
});
