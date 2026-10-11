const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module');
const ts = require('typescript'), React = require('react');
const {create, act} = require('react-test-renderer');
const root = path.resolve(__dirname, '..');
const deferred = () => { let resolve; const promise = new Promise(r => {resolve = r;}); return {promise, resolve}; };
const text = node => Array.isArray(node) ? node.map(text).join('') : typeof node === 'string' ? node : (node?.children || []).map(text).join('');
function production(filename, invoke, cache = new Map()) {
  filename = path.resolve(root, filename);
  if (cache.has(filename)) return cache.get(filename).exports;
  const mod = new Module(filename, module); cache.set(filename, mod);
  mod.paths = Module._nodeModulePaths(path.dirname(filename)); const original = mod.require.bind(mod);
  mod.require = id => {
    if (id === '@tauri-apps/api/core') return {invoke, Channel: class {}};
    if (id.startsWith('./')) {
      const candidate = path.resolve(path.dirname(filename), id);
      const target = ['.ts', '.tsx'].map(ext => candidate + ext).find(p => fs.existsSync(p));
      if (target) return production(target, invoke, cache);
    }
    return original(id);
  };
  mod._compile(ts.transpileModule(fs.readFileSync(filename, 'utf8'), {compilerOptions:{module:ts.ModuleKind.CommonJS, jsx:ts.JsxEmit.ReactJSX, target:ts.ScriptTarget.ES2022}}).outputText, filename);
  return mod.exports;
}
const profile = {id:'fixture',name:'Startup fixture',enabled:true,engine:'srbminer',algorithm:'fixture',pool:'example.invalid',gpu_ids:'0',wallet:'public-fixture',dev_tip_percent:0};
const runtime = {id:'fixture',running:false,pid:null,started_at:null,uptime_seconds:0,started_by:'',last_error:'',metrics:{hashrate_hps:null,power_w:null,temperature_c:null,accepted:0,rejected:0},dev_tip_active:false};
const config = {miners:[profile],schedules:[{id:'window',enabled:true,miner_ids:['fixture']}],web:{desktop_api_port:17936,token:''},ui:{language:'en',start_with_windows:false,close_to_tray:false}};
const nativeStatus = {supported:true,reachable:true,port:17936,task_installed:true,task_state:'Running',process_running:true,log_tail:''};

for (const [desktop, statusHasSchedule] of [[true,true],[true,undefined],[true,false],[false,true]]) test(`${desktop ? 'Windows' : 'Browser'} startup survives status arriving before native/config responses (has_schedule=${statusHasSchedule})`, async () => {
  const native = deferred(), configuration = deferred(), storage = new Map([['minerdesk.web.port','17936']]);
  const calls = [], timers = [], splash = {hidden:false,removed:false}, failures = [];
  const elements = {'boot-status':{textContent:''},'boot-splash':{classList:{add(){splash.hidden=true;}},remove(){splash.removed=true;}}};
  global.localStorage = {getItem:k => storage.get(k) ?? null,setItem:(k,v) => storage.set(k,v)};
  global.location = {protocol:'http:',search:''};
  global.window = {...(desktop ? {__TAURI_INTERNALS__:{}} : {}),setTimeout:fn => {timers.push(fn);return timers.length;},clearTimeout(){},setInterval(){return 1;},clearInterval(){},addEventListener(){},removeEventListener(){}};
  global.document = {visibilityState:'visible',documentElement:{},getElementById:id => elements[id] ?? null};
  const previousFetch = global.fetch;
  global.fetch = async (url, options) => {
    const route = new URL(url,'http://127.0.0.1:17936').pathname;calls.push([route,options?.method || 'GET']);
    assert.equal(options?.method || 'GET','GET','Startup must not mutate configuration or mining');
    let body;
    if (route === '/api/config') body = await configuration.promise;
    else if (route === '/api/status') body = [{profile,runtime,scheduled_now:false,has_schedule:statusHasSchedule}];
    else if (route === '/api/health') body = {version:require('../package.json').version,platform:'windows',headless:true};
    else if (route === '/api/engines') body = [{id:'srbminer',name:'SRBMiner-Multi'}];
    else if (route === '/api/power/pending') body = null;
    else if (route === '/api/security/status') body = {supported:true};
    else throw Error(`Unexpected startup request: ${route}`);
    return new Response(JSON.stringify(body),{status:200,headers:{'Content-Type':'application/json'}});
  };
  const App = production('src/App.tsx', command => {
    assert.equal(command,'get_backend_status');return native.promise;
  }).default;
  class Boundary extends React.Component {
    state = {error:null};
    static getDerivedStateFromError(error){return {error};}
    componentDidCatch(error){failures.push(error.message);}
    render(){return this.state.error ? React.createElement('div',null,'Render failed') : this.props.children;}
  }
  let view;
  try {
    await act(async () => {view = create(React.createElement(Boundary,null,React.createElement(App)));});
    assert(calls.some(([route]) => route === '/api/status'));
    assert.equal(elements['boot-status'].textContent,desktop ? 'Checking MinerDesk backend…' : 'Connecting to MinerDesk backend…');
    assert.deepEqual(failures,[], 'Status polling must render safely while configuration is null');
    assert.match(text(view.toJSON()),/Startup fixture/);
    assert.equal(splash.hidden,false);
    assert(view.root.findAllByType('button').filter(b => ['Start','Resume schedule','Force mining','Start all'].includes(text(b))).every(b => b.props.disabled), 'Mining controls must wait for a connected backend');
    // The native response arrives next; HTTP config is still pending.
    await act(async () => {native.resolve(nativeStatus);});
    assert.deepEqual(failures,[]);
    assert.equal(splash.hidden,false);
    await act(async () => {configuration.resolve(config);});
    assert.deepEqual(failures,[]);
    assert.equal(splash.hidden,true, 'Successful bootstrap must remove the loading overlay');
    assert.match(text(view.toJSON()),/waiting for scheduled window/);
    assert(view.root.findAllByType('button').some(b => text(b)==='Force mining' && !b.props.disabled));
  } finally {
    if (view) await act(async () => view.unmount());
    native.resolve(nativeStatus);configuration.resolve(config);global.fetch=previousFetch;
  }
});
