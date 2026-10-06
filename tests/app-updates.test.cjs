const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const React = require('react');
const {create, act} = require('react-test-renderer');
const root = path.resolve(__dirname,'..');
let calls = [], nativeUpdate, installError = '', preferences, timers, webCalls, installing;
function production(name) {
  const filename = path.join(root, name);
  const mod = new Module(filename, module);
  mod.paths = Module._nodeModulePaths(path.dirname(filename));
  const original = mod.require.bind(mod);
  mod.require = id => {
    if (id === '@tauri-apps/api/core') return {
      Channel: class { onmessage() {} },
      invoke: async (command, args) => {
        calls.push([command,args]);
        if (command === 'check_app_update') return nativeUpdate;
        if (command === 'install_app_update') { args.progress.onmessage({stage:'downloading',downloaded:40,total:100}); if (installError) throw Error(installError); }
      },
    };
    if (id.startsWith('./')) return production('src/' + id.slice(2) + '.ts');
    return original(id);
  };
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText, filename);
  return mod.exports;
}
const policy = production('src/updatePolicy.ts');
const AppUpdates = production('src/AppUpdates.tsx').default;
const fresh = () => ({current_version:'0.7.24',version:'0.7.25',notes:'Verified release',release_url:'https://github.com/jontechlabs/MinerDesk/releases/tag/v0.7.25',can_install:true,reason:'supported'});
function setup() {
  calls=[]; nativeUpdate=fresh(); installError=''; preferences=new Map(); timers=[]; webCalls=0; installing=[];
  global.localStorage={getItem:k=>preferences.get(k)??null,setItem:(k,v)=>preferences.set(k,v)};
  global.window={setTimeout:fn=>{timers.push(fn);return timers.length;},setInterval:()=>99,clearTimeout:()=>{},clearInterval:()=>{}};
}
function props(extra={}) { return {desktop:true,language:'en',settings:true,dirty:false,busy:false,checkWeb:async()=>{webCalls++;return {...fresh(),can_install:false,reason:'web'};},onInstalling:value=>installing.push(value),...extra}; }
function text(node) { return Array.isArray(node) ? node.map(text).join('') : typeof node === 'string' ? node : (node?.children || []).map(text).join(''); }
function button(view,label) { return view.root.findAllByType('button').find(b=>text(b)===label); }
async function check(view) { await act(async()=>button(view,'Check now').props.onClick()); }

test('periodic checks and version-specific snooze survive clock changes',()=>{
  const {shouldCheckUpdate:check,shouldNotifyUpdate:notify,UPDATE_INTERVAL_MS:interval}=policy;
  assert.equal(check(false,0,1),false); assert.equal(check(true,0,1),true);
  assert.equal(check(true,100,100+interval-1),false); assert.equal(check(true,100,100+interval),true);
  assert.equal(check(true,1000,500),true);
  assert.equal(notify(fresh(),'0.7.25',2000,1000),false);
  assert.equal(notify(fresh(),'0.7.24',2000,1000),true);
  assert.equal(notify({...fresh(),version:null},'',0,1000),false);
});
test('release links cannot navigate to a foreign site or injected URL',()=>{
  assert.equal(policy.safeReleaseUrl(fresh().release_url),fresh().release_url);
  for(const url of ['javascript:alert(1)','https://github.com/evil/MinerDesk/releases/tag/v1.0.0',fresh().release_url+'?x=1']) assert.equal(policy.safeReleaseUrl(url),policy.UPDATE_RELEASES_URL);
});
test('finding an update never installs without the explicit confirmation button',async()=>{
  setup(); let view; await act(async()=>{view=create(React.createElement(AppUpdates,props()));});
  await check(view);
  assert.equal(calls.filter(c=>c[0]==='install_app_update').length,0);
  await act(async()=>button(view,'Update…').props.onClick());
  assert.equal(calls.length,1); assert(button(view,'Download, install & restart'));
  await act(async()=>button(view,'Cancel').props.onClick());
  assert.equal(calls.length,1);
  await act(async()=>view.unmount());
});
test('unsaved changes block installation even if handler is called',async()=>{
  setup(); let view; await act(async()=>{view=create(React.createElement(AppUpdates,props({dirty:true})));});
  await check(view); await act(async()=>button(view,'Update…').props.onClick());
  const install=button(view,'Download, install & restart'); assert.equal(install.props.disabled,true);
  await act(async()=>install.props.onClick());
  assert(!calls.some(c=>c[0]==='install_app_update')); await act(async()=>view.unmount());
});
test('signature/download failure leaves an actionable error and clears busy state',async()=>{
  setup(); installError='Signature verification failed'; let view;
  await act(async()=>{view=create(React.createElement(AppUpdates,props()));}); await check(view);
  await act(async()=>button(view,'Update…').props.onClick());
  await act(async()=>button(view,'Download, install & restart').props.onClick());
  assert.equal(calls.filter(c=>c[0]==='install_app_update').length,1); assert.deepEqual(installing,[true,false]);
  assert.match(text(view.toJSON()),/Signature verification failed/); await act(async()=>view.unmount());
});
test('headless browser only notifies and opens GitHub downloads',async()=>{
  setup(); let view; await act(async()=>{view=create(React.createElement(AppUpdates,props({desktop:false})));}); await check(view);
  assert.equal(webCalls,1); assert.equal(calls.length,0); assert.equal(button(view,'Update…'),undefined);
  assert(view.root.findAllByType('a').some(a=>a.props.href===fresh().release_url)); await act(async()=>view.unmount());
});
test('automatic checks can be disabled while manual checks remain available',async()=>{
  setup(); preferences.set('minerdesk.updates.enabled','false'); let view;
  await act(async()=>{view=create(React.createElement(AppUpdates,props()));});
  await act(async()=>{await timers[0]();}); assert.equal(calls.length,0);
  await check(view); assert.equal(calls.length,1);
  await act(async()=>button(view,'Later (24 hours)').props.onClick());
  assert.equal(preferences.get('minerdesk.updates.snoozedVersion'),'0.7.25'); await act(async()=>view.unmount());
});
