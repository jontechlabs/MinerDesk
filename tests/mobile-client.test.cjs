// Exercise production transport and React bootstrap with a fake native bridge.
// No real rig, OS authorization, HTTP listener or mobile keystore is touched.
const test = require('node:test'), assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module');
const ts = require('typescript'), React = require('react'), {create, act} = require('react-test-renderer');
const root = path.resolve(__dirname, '..');
function production(file, invoke = () => {throw Error('Unexpected native command');}, appStub = null, cache = new Map()) {
  const filename = path.resolve(root, file);
  if (cache.has(filename)) return cache.get(filename).exports;
  const mod = new Module(filename, module); cache.set(filename, mod); mod.paths = Module._nodeModulePaths(path.dirname(filename));
  const original = mod.require.bind(mod);
  mod.require = id => {
    if (id === '@tauri-apps/api/core') return {invoke, Channel:class{}};
    if (appStub && id === './App') return {default:appStub};
    if (id.startsWith('./')) {
      const base = path.resolve(path.dirname(filename), id);
      const target = ['.ts','.tsx'].map(ext => base+ext).find(candidate => fs.existsSync(candidate));
      if (target) return production(target, invoke, appStub, cache);
    }
    return original(id);
  };
  mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8'), {fileName:filename,compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX}}).outputText, filename);
  return mod.exports;
}
const text = node => Array.isArray(node) ? node.map(text).join('') : typeof node === 'string' ? node : (node?.children || []).map(text).join('');
function environment(mobile = true) {
  const storage = new Map([['minerdesk.language','en']]);
  global.localStorage = {getItem:key => storage.get(key) ?? null,setItem:(key,value) => storage.set(key,value)};
  global.location = {protocol:'http:',hostname:'tauri.localhost',search:'?token=must-not-be-used'};
  global.window = {__TAURI_INTERNALS__:{},...(mobile ? {__MINERDESK_MOBILE__:true} : {}),setTimeout(){return 1;},clearTimeout(){},setInterval(){return 1;},clearInterval(){},addEventListener(){},removeEventListener(){}};
  global.document = {visibilityState:'visible',documentElement:{},getElementById(){return null;}};
  return storage;
}

test('mobile Tauri never selects desktop backend startup while desktop Tauri is unchanged', () => {
  environment(); assert.equal(production('src/runtime.ts').isDesktopTauri,false);
  assert.equal(production('src/runtime.ts').isMobileClient,true);
  environment(false); assert.equal(production('src/runtime.ts').isDesktopTauri,true);
  delete global.window.__TAURI_INTERNALS__; assert.equal(production('src/runtime.ts').isDesktopTauri,false);
});

test('a late save→start continuation retains its original connection and cannot target the new rig', async () => {
  const {createNativeJsonClient} = production('src/api.ts');
  const calls=[]; let active='old-session', releaseSave;
  const save = new Promise(resolve => {releaseSave=resolve;});
  const bridge = async args => {
    calls.push(args);
    if (args.connectionId !== active) throw Error('Connection changed; choose this rig again');
    if (args.path === '/api/config') await save;
    return {ok:true};
  };
  const old = createNativeJsonClient(bridge,'old-session');
  const continuation = (async()=>{await old('/api/config',{method:'PUT',body:'{}'},'must-not-forward');await old('/api/miners/example/start',{method:'POST'});})();
  active='new-session';
  const current = createNativeJsonClient(bridge,'new-session');
  assert.deepEqual(await current('/api/health'),{ok:true}); releaseSave();
  await assert.rejects(continuation,/Connection changed/);
  assert.deepEqual(calls.map(c=>[c.connectionId,c.path]),[['old-session','/api/config'],['new-session','/api/health'],['old-session','/api/miners/example/start']]);
  assert(calls.every(c=>!('headers' in c)&&!('token' in c)));
});

test('native transport rejects absolute URLs, invalid methods/bodies, and pre-aborted requests', async () => {
  const {createNativeJsonClient} = production('src/api.ts'); let calls=0;
  const client=createNativeJsonClient(async()=>{calls++;return{};},'session');
  for (const path of ['https://example.com/api/health','//example.com/api/health','/api/health#fragment','/api/health\r\nX-Test: yes']) await assert.rejects(client(path));
  await assert.rejects(client('/api/config',{method:'PATCH',body:'{}'}));
  await assert.rejects(client('/api/config',{method:'POST',body:new FormData()}));
  const controller=new AbortController();controller.abort();
  await assert.rejects(client('/api/health',{signal:controller.signal}),{name:'AbortError'});
  assert.equal(calls,0);
});

test('native timeout and caller cancellation never retry an uncertain write', async () => {
  const {createNativeJsonClient,ApiTimeoutError} = production('src/api.ts'); let calls=0;
  const client=createNativeJsonClient(()=>{calls++;return new Promise(()=>{});},'session');
  await assert.rejects(client('/api/miners/example/start',{method:'POST'},'',20),ApiTimeoutError);
  const controller=new AbortController();const pending=client('/api/config',{method:'PUT',body:'{}',signal:controller.signal});controller.abort();
  await assert.rejects(pending,{name:'AbortError'});assert.equal(calls,2);
});

test('connection chooser remembers only the sanitized address and binds the returned native identity', async () => {
  const storage=environment();const calls=[];let mountedId;
  const AppStub=props=>{mountedId=props.mobileConnectionId;return React.createElement('div',null,'Connected fixture');};
  const Connection=production('src/MobileConnection.tsx',async(command,args)=>{
    calls.push({command,args});
    if(command==='mobile_connection_status')return{connected:false};
    if(command==='mobile_connect')return{connected:true,connectionId:'opaque-session',address:'https://rig.example/?token=server-secret',name:'Test rig',version:'0.8.2'};
    if(command==='mobile_disconnect')return;
    throw Error(command);
  },AppStub).default;
  let view;try {
    await act(async()=>{view=create(React.createElement(Connection));});
    await act(async()=>{
      view.root.findAllByType('input').find(i=>i.props.type==='url').props.onChange({target:{value:'https://rig.example/?token=pasted-secret'}});
      view.root.findAllByType('input').find(i=>i.props.type==='password').props.onChange({target:{value:'typed-secret'}});
    });
    await act(async()=>{view.root.findByType('form').props.onSubmit({preventDefault(){}});});
    assert.equal(mountedId,'opaque-session');assert.equal(storage.get('minerdesk.mobile.address'),'https://rig.example');
    assert(![...storage.values()].some(value=>/secret|token=/.test(value)));
    const connect=calls.find(c=>c.command==='mobile_connect');assert.equal(connect.args.key,'typed-secret');
    await act(async()=>view.root.findAllByType('button').find(b=>text(b)==='Connections').props.onClick());
    assert.match(text(view.toJSON()),/Commands already sent may still finish/);
    await act(async()=>view.root.findAllByType('button').find(b=>text(b)==='Disconnect').props.onClick());
    assert(calls.some(c=>c.command==='mobile_disconnect'));
    assert.equal(view.root.findAllByType('input').find(i=>i.props.type==='password').props.value,'');
  } finally {if(view)await act(async()=>view.unmount());}
});

test('mobile bootstrap uses only its bound native API and never persists rig credentials or offers local administration', async () => {
  const storage=environment();storage.set('minerdesk.settings.v0.3','{"prlWallet":"must-not-migrate"}');
  const calls=[];
  const profile={id:'fixture',name:'Mobile rig miner',enabled:true,engine:'srbminer',algorithm:'pearlhash',pool:'pool.example',wallet:'public-wallet',gpu_ids:'0',gpu_tuning:[],linux_run_as_root:true};
  const config={miners:[profile],schedules:[],web:{desktop_api_port:17888,expose_lan:true,token:'rig-access-secret'},ui:{language:'fr',start_with_windows:true,close_to_tray:true}};
  const status={profile,runtime:{id:'fixture',running:false,uptime_seconds:0,started_by:'',last_error:'',metrics:{hashrate_hps:1e12,power_w:120,temperature_c:60,accepted:1,rejected:0},dev_tip_active:false},has_schedule:false};
  const previousFetch=global.fetch;global.fetch=()=>{throw Error('A phone must not use localhost browser HTTP');};
  const App=production('src/App.tsx',async(command,args)=>{
    calls.push({command,args});assert.equal(command,'mobile_request');assert.equal(args.connectionId,'fixed-session');assert.equal(args.method,'GET');
    if(args.path==='/api/health')return{version:'99.0.0',platform:'linux',headless:true,linux_elevation_available:true};
    if(args.path==='/api/config')return config;
    if(args.path==='/api/engines')return[{id:'srbminer',name:'SRBMiner'}];
    if(args.path==='/api/status')return[status];
    throw Error('Unexpected mobile request: '+args.path);
  }).default;
  let view;try {
    await act(async()=>{view=create(React.createElement(App,{mobileConnectionId:'fixed-session'}));});
    assert.match(text(view.toJSON()),/Mobile rig miner/);assert.match(text(view.toJSON()),/Mobile controller/);
    assert(![...storage.values()].some(value=>value.includes('rig-access-secret')||value.includes('must-not-be-used')));
    assert(!calls.some(c=>/security|power|updates/.test(c.args.path)));
    await act(async()=>view.root.findAllByType('button').find(b=>text(b)==='Start').props.onClick());
    assert.match(text(view.toJSON()),/Authorize root mining in MinerDesk on the rig first/);
    assert(!calls.some(c=>c.args.path.endsWith('/start')));
    await act(async()=>view.root.findAllByType('button').find(b=>b.props['aria-label']==='Settings').props.onClick());
    assert(!text(view.toJSON()).includes('Windows Security'));assert(!text(view.toJSON()).includes('rig-access-secret'));
    assert(!text(view.toJSON()).includes('New access token'));
  } finally {if(view)await act(async()=>view.unmount());global.fetch=previousFetch;}
});
