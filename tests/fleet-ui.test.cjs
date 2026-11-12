const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module');
const ts = require('typescript'), React = require('react'), { create, act } = require('react-test-renderer');
const modules = new Map();
function production(name) {
  const filename = path.resolve(__dirname, '../src', name);
  if (modules.has(filename)) return modules.get(filename).exports;
  const mod = new Module(filename, module); modules.set(filename, mod);
  mod.paths = Module._nodeModulePaths(path.dirname(filename));
  const original = mod.require.bind(mod);
  mod.require = id => id.startsWith('./') ? production(id.slice(2) + '.ts') : original(id);
  mod._compile(ts.transpileModule(fs.readFileSync(filename, 'utf8'), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022,
  } }).outputText, filename);
  return mod.exports;
}
const Fleet = production('Fleet.tsx').default;
const { decodeFleetView, decodeFleetLogs, fleetSummary, fleetPreflight, fleetEditablePatch, fleetRigFresh } = production('fleetTypes.ts');
const text = n => Array.isArray(n) ? n.map(text).join('') : typeof n === 'string' ? n : (n?.children || []).map(text).join('');
function miner(id, algorithm = 'pearlhash', running = false) { return {
  id, name: 'Profile ' + id, engine: 'srbminer', algorithm, enabled: true, gpu_ids: '0', running,
  mode: running ? 'schedule' : 'waiting', scheduled_now: running, has_schedule: true, scheduler_paused: false,
  uptime_seconds: 10, metrics: { hashrate_hps: running ? 1e12 : null, power_w: running ? 100 : null, temperature_c: null, fan_pct: null, accepted: 1, rejected: 0 },
  last_error: '', root_authorization: 'not_requested',
}; }
function profile(id) { return { id, revision: 'profile-rev-' + id, name: 'Profile ' + id, engine: 'srbminer', enabled: true,
  algorithm: 'pearlhash', pool: 'pool.example:3360', wallet: 'public-fixture', secondary_wallet: '', merge_secondary: false, merge_separator: '+',
  worker: 'fixture', gpu_ids: '0', gpu_tuning: [], core_clock: null, memory_clock: null, core_offset: null, memory_offset: null, power_limit: null, fan: null,
  disable_cpu: true, allow_gpu_overlap: false, linux_run_as_root: false, root_authorization: 'not_requested' }; }
function rig(id, extra = {}) { return {
  rig_id: id, name: 'Rig ' + id, last_seen: Math.floor(Date.now()/1000), online: true,
  snapshot: { health: 'online', name: 'Rig ' + id, os: 'linux', arch: 'x86_64', version: 'test', local_time: '2026-11-12T16:00:00+04:00', timezone: '+04:00', utc_offset_seconds: 14400,
    revision: 'snapshot-' + id, status: [miner(id)], gpu_inventory: [], gpu_inventory_available: false },
  config: { revision: 'config-' + id, profiles: [profile(id)], schedules: [], local_time: '2026-11-12T16:00:00+04:00', timezone: '+04:00', utc_offset_seconds: 14400,
    editable_profile_fields: ['name','pool','wallet','core_clock','memory_clock','power_limit','enabled'] }, ...extra,
}; }
function view(rigs = [], extra = {}) { return {
  setup: { rig_id: 'local', name: 'Local', mode: 'hub', hub_url: 'https://192.168.1.10:17889', fleet_id: 'fixture', hub_enabled: true, tls_fingerprint: 'fixture-fingerprint', error: null },
  rigs, commands: [], local_setup_allowed: true, controller_allowed: true, ...extra,
}; }
async function render(data, request) {
  const calls = []; let instance;
  const api = request || (async (p,o) => { calls.push({p,o}); if (p === '/api/fleet') return data; throw new Error('Unexpected request ' + p); });
  await act(async () => { instance = create(React.createElement(Fleet, { language:'en', request:api })); await Promise.resolve(); });
  return { instance, calls };
}
function button(instance, label) { return instance.root.findAllByType('button').find(b=>text(b)===label); }
async function click(instance, label) { const b=button(instance,label); assert(b, 'Missing button '+label); await act(async()=>{await b.props.onClick();await Promise.resolve();}); }

test('current totals exclude stale and unreachable rigs and keep algorithms separate', () => {
  const prl = rig('prl'); prl.snapshot.status = [miner('p','pearlhash',true)];
  const qtc = rig('qtc'); qtc.snapshot.status = [miner('q','quantus',true)]; qtc.snapshot.status[0].metrics.hashrate_hps=2e9;
  const offline = rig('off',{online:false}); offline.snapshot.status=[miner('o','pearlhash',true)];
  const stale = rig('stale',{last_seen:Math.floor(Date.now()/1000)-90}); stale.snapshot.status=[miner('s','pearlhash',true)];
  const result=fleetSummary([prl,qtc,offline,stale]);
  assert.equal(result.online,2); assert.equal(result.running,2); assert.equal(result.unknown,2); assert.equal(result.power,200);
  assert.deepEqual(result.algorithms,{pearlhash:1e12,quantus:2e9}); assert.equal(fleetRigFresh(stale),false);
});

test('preflight distinguishes offline, disabled profiles and required local root approval', () => {
  const r=rig('linux'); r.snapshot.status[0].root_authorization='required';
  assert.equal(fleetPreflight(r,'linux','start'),'local_authorization');
  assert.equal(fleetPreflight(r,'linux','stop'),null);
  r.online=false; assert.equal(fleetPreflight(r,'linux','stop'),'offline');
  r.online=true;r.snapshot.status[0].root_authorization='not_requested';r.snapshot.status[0].enabled=false;
  assert.equal(fleetPreflight(r,'linux','restart'),'disabled');
});

test('profile patch intersects the server allowlist and never includes paths or grants', () => {
  const r=rig('edit'),before=r.config.profiles[0];
  const after={...before,name:'Changed',pool:'new.example:1',engine:'custom',linux_run_as_root:true,executable_path:'/tmp/untrusted',linux_root_authorization:{id:'grant'}};
  r.config.editable_profile_fields.push('engine','linux_run_as_root','executable_path','linux_root_authorization');
  assert.deepEqual(fleetEditablePatch(r.config,before,after),{name:'Changed',pool:'new.example:1'});
});

test('first install has no fictitious rigs and honors server-provided local access gates', async () => {
  const data=view([],{local_setup_allowed:false,controller_allowed:false,setup:{...view().setup,mode:'standalone'}});
  const {instance,calls}=await render(data);
  try {
    assert.match(text(instance.toJSON()),/Your fleet starts here/);
    assert.match(text(instance.toJSON()),/Pairing and coordinator setup require authorized local access/);
    assert.equal(button(instance,'Create fleet').props.disabled,true);
    assert.equal(button(instance,'Scan LAN').props.disabled,true);
    assert.equal(calls.filter(c=>c.p==='/api/fleet/discover').length,0);
    assert(!text(instance.toJSON()).includes('RTX 5060'));
  } finally { await act(async()=>instance.unmount()); }
});

test('joining needs only the coordinator URL and pairing key, with no personal machine name', async () => {
  let data=view([],{setup:{...view().setup,name:'',mode:'standalone'}});const calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;
    if(p==='/api/fleet/join'){data=view([rig('detected-host')],{setup:{...view().setup,mode:'member',name:'detected-host'}});return data;}
    throw new Error('Unexpected '+p);
  };
  const {instance}=await render(data,request);
  try {
    const form=instance.root.findAllByType('form').find(f=>f.findAllByType('button').some(b=>text(b)==='Join'));
    const inputs=form.findAllByType('input');
    assert.deepEqual(inputs.map(i=>i.props.type),['url','password']);
    assert(inputs.every(i=>i.props.required));assert.equal(button(instance,'Join').props.disabled,true);
    await act(async()=>{
      inputs[0].props.onChange({target:{value:'https://192.168.1.10:17889'}});
      inputs[1].props.onChange({target:{value:' pairing-fixture '}});
    });
    assert.equal(button(instance,'Join').props.disabled,false);
    await act(async()=>{form.props.onSubmit({preventDefault(){}});await Promise.resolve();});
    const sent=calls.filter(c=>c.p==='/api/fleet/join');assert.equal(sent.length,1);
    assert.deepEqual(JSON.parse(sent[0].o.body),{address:'https://192.168.1.10:17889',key:'pairing-fixture'});
    assert.match(text(instance.toJSON()),/Rig detected-host/);
    assert(!instance.root.findAllByType('input').some(i=>i.props.type==='password'));
  } finally {await act(async()=>instance.unmount());}
});

test('Open shows actual dashboard telemetry, pool and GPU assignments, retaining zero versus unknown', async () => {
  const r=rig('telemetry');
  const active=miner('telemetry','pearlhash',true);
  Object.assign(active,{uptime_seconds:90061,dev_tip_active:true});
  active.metrics={hashrate_hps:123456789,power_w:183.6,temperature_c:67.8,fan_pct:42,accepted:321,rejected:7};
  const unknown=miner('unknown','quantus',true);unknown.uptime_seconds=0;
  unknown.metrics={hashrate_hps:null,power_w:null,temperature_c:null,fan_pct:null,accepted:0,rejected:0};
  const zero=miner('zero');zero.uptime_seconds=0;zero.metrics={hashrate_hps:0,power_w:0,temperature_c:0,fan_pct:0,accepted:0,rejected:0};
  r.snapshot.status=[active,unknown,zero];r.config.profiles.push(profile('unknown'),profile('zero'));
  const {instance}=await render(view([r]));
  try {
    await click(instance,'Open');
    const table=instance.root.findByProps({className:'fleet-table fleet-miner-table'});
    const headings=table.findAllByType('th').map(text);
    assert.deepEqual(headings,['Profile','Engine','Algo / GPU','State','Hashrate','W','°C','Fan %','A/R','Uptime','Actions']);
    const rows=table.findByType('tbody').findAllByType('tr').map(row=>row.findAllByType('td').map(text));
    assert.match(rows[0][0],/pool\.example:3360/);assert.equal(rows[0][1],'srbminer');assert.equal(rows[0][2],'pearlhashGPU 0');
    assert.match(rows[0][3],/Within schedule/);assert.match(rows[0][3],/Developer tip active/);
    assert.deepEqual(rows[0].slice(4,10),['123 MH/s','184','68','42','321/7','25:01:01']);
    assert.deepEqual(rows[1].slice(4,10),['—','—','—','—','0/0','00:00:00']);
    assert.deepEqual(rows[2].slice(4,10),['0 H/s','0','0','0','0/0','00:00:00']);
    const summary=instance.root.findByProps({'aria-label':'Rig summary'});
    assert.deepEqual(summary.findAllByProps({className:'fleet-kpi-value'}).map(text),['2 / 3','184 W','68 °C','321 / 7']);
    assert(!text(table).includes('public-fixture'));assert(!text(table).includes('wallet'));
  } finally {await act(async()=>instance.unmount());}
});

test('the selected dashboard follows automatic polls and suppresses metrics when the rig becomes stale', async () => {
  const originalSetTimeout=global.setTimeout,originalClearTimeout=global.clearTimeout,polls=[];
  const timers=new Set();let instance;
  global.setTimeout=(callback,delay,...args)=>{
    if(delay!==5000)return originalSetTimeout(callback,delay,...args);
    const timer={callback:()=>callback(...args)};timers.add(timer);polls.push(timer);return timer;
  };
  global.clearTimeout=timer=>{if(!timers.delete(timer))originalClearTimeout(timer);};
  let data=view([rig('live')]);data.rigs[0].snapshot.status=[miner('live','pearlhash',true)];
  const calls=[],request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;throw new Error(p);};
  const readings=()=>instance.root.findByProps({className:'fleet-table fleet-miner-table'}).findByType('tbody').findAllByType('td').slice(4,10).map(text);
  const poll=async()=>{const next=polls.shift();assert(next,'Missing scheduled Fleet refresh');timers.delete(next);await act(async()=>{next.callback();await Promise.resolve();});};
  try {
    ({instance}=await render(data,request));await click(instance,'Open');
    assert.deepEqual(readings(),['1.00 TH/s','100','—','—','1/0','00:00:10']);
    data=structuredClone(data);const active=data.rigs[0].snapshot.status[0];
    active.metrics={hashrate_hps:2250000,power_w:202,temperature_c:71,fan_pct:58,accepted:47,rejected:2};active.uptime_seconds=3661;
    await poll();
    assert.deepEqual(readings(),['2.25 MH/s','202','71','58','47/2','01:01:01']);
    assert(button(instance,'Close details'));assert.match(text(instance.root.findByProps({'aria-label':'Rig summary'})),/47 \/ 2/);
    data=structuredClone(data);data.rigs[0].last_seen=Math.floor(Date.now()/1000)-90;
    await poll();
    assert.deepEqual(readings(),['—','—','—','—','—','—']);
    assert.match(text(instance.toJSON()),/Current mining is unknown/);
    assert.equal(button(instance,'Stop').props.disabled,true);
    assert.equal(calls.filter(c=>c.p==='/api/fleet').length,3);assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
  } finally {
    if(instance)await act(async()=>instance.unmount());
    global.setTimeout=originalSetTimeout;global.clearTimeout=originalClearTimeout;
  }
});

test('current dashboard totals exclude stale miners and preserve reported zero readings', () => {
  const zero=rig('zero');zero.snapshot.status=[miner('zero','pearlhash',true)];
  zero.snapshot.status[0].metrics={hashrate_hps:0,power_w:0,temperature_c:0,fan_pct:0,accepted:0,rejected:0};
  const stale=rig('stale',{last_seen:Math.floor(Date.now()/1000)-90});stale.snapshot.status=[miner('stale','quantus',true)];
  stale.snapshot.status[0].metrics={hashrate_hps:999,power_w:999,temperature_c:999,fan_pct:99,accepted:999,rejected:999};
  const result=fleetSummary([zero,stale]);
  assert.equal(result.activeMiners,1);assert.equal(result.power,0);assert.equal(result.temperature,0);assert.equal(result.accepted,0);assert.equal(result.rejected,0);
  const unknown=rig('unknown');unknown.snapshot.status=[miner('unknown','pearlhash',true)];
  unknown.snapshot.status[0].metrics.power_w=null;
  assert.equal(fleetSummary([unknown]).power,null);assert.equal(fleetSummary([unknown]).temperature,null);
  const offline=fleetSummary([stale]);assert.equal(offline.accepted,null);assert.equal(offline.rejected,null);
});

test('bulk review sends only ready rigs and does not silently force schedules', async () => {
  const good=rig('ready'),off=rig('offline',{online:false}),root=rig('root'); root.snapshot.status[0].root_authorization='required';
  const data=view([good,off,root]),calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;if(p==='/api/fleet/commands')return [{id:'receipt',rig_id:'ready',created_at:0,expires_at:9999999999,state:'queued',result:null,error:null}];throw new Error(p);};
  const {instance}=await render(data,request);
  try {
    await act(async()=>{for(const c of instance.root.findAllByType('input').filter(x=>x.props.type==='checkbox'))c.props.onChange({target:{checked:true}});});
    await click(instance,'Bulk actions…');
    assert.match(text(instance.toJSON()),/Root approval required on the rig/);assert.match(text(instance.toJSON()),/Offline: no command will be sent/);
    assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
    await click(instance,'Confirm ready rigs');
    const sent=calls.filter(c=>c.p==='/api/fleet/commands').map(c=>JSON.parse(c.o.body));
    assert.deepEqual(sent,[{rig_ids:['ready'],operation:{action:'start',profile_id:'ready'}}]);
    assert.match(text(instance.toJSON()),/Queued/);assert.match(text(instance.toJSON()),/not a confirmed success/);
  } finally { await act(async()=>instance.unmount()); }
});

test('force requires an explicit review, sends the typed force action and GPU inventory remains honest', async () => {
  const r=rig('local'),data=view([r]),calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;if(p==='/api/fleet/commands')return [{id:'force-receipt',rig_id:'local',state:'succeeded',result:{ok:true},error:null}];throw new Error(p);};
  const {instance}=await render(data,request);
  try {
    await click(instance,'Rig local'); await click(instance,'GPU');
    assert.match(text(instance.toJSON()),/no verified cached GPU inventory/);assert(!text(instance.toJSON()).includes('GPU 0'));
    await click(instance,'Profiles');await click(instance,'Force…');
    assert.match(text(instance.toJSON()),/Explicit override/);assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
    await click(instance,'Confirm ready rigs');
    assert.equal(JSON.parse(calls.find(c=>c.p==='/api/fleet/commands').o.body).operation.action,'force_start');
  } finally { await act(async()=>instance.unmount()); }
});

test('remote profile save carries the reviewed profile revision and only changed fields', async () => {
  const r=rig('edit'),data=view([r]),calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;if(p==='/api/fleet/commands')return [{id:'edit-receipt',rig_id:'edit',state:'queued',result:null,error:null}];throw new Error(p);};
  const {instance}=await render(data,request);
  try {
    await click(instance,'Rig edit');await click(instance,'Edit');
    const name=instance.root.findAllByType('input').find(i=>i.props.value==='Profile edit');
    await act(async()=>name.props.onChange({target:{value:'Reviewed name'}}));
    await click(instance,'Save this revision');
    const payload=JSON.parse(calls.find(c=>c.p==='/api/fleet/commands').o.body);
    assert.deepEqual(payload,{rig_ids:['edit'],operation:{action:'profile_patch',profile_id:'edit',expected_revision:'profile-rev-edit',patch:{name:'Reviewed name'}}});
  } finally { await act(async()=>instance.unmount()); }
});

test('pairing keys are shown only from the explicit invitation response and never persisted', async () => {
  const data=view([rig('local')]),calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;if(p==='/api/fleet/invite')return {address:'https://192.168.1.10:17889',key:'ONE-TIME-FIXTURE',expires_at:Math.floor(Date.now()/1000)+300,certificate_fingerprint:'public-fingerprint'};throw new Error(p);};
  const {instance}=await render(data,request);
  try {
    assert(!text(instance.toJSON()).includes('ONE-TIME-FIXTURE'));
    await click(instance,'Pair a rig');assert.match(text(instance.toJSON()),/ONE-TIME-FIXTURE/);
    await click(instance,'Close and hide key');assert(!text(instance.toJSON()).includes('ONE-TIME-FIXTURE'));
    assert.equal(calls.filter(c=>c.p==='/api/fleet/invite').length,1);
  } finally { await act(async()=>instance.unmount()); }
});

test('a pending fleet poll is never overlapped and unmount aborts its request', async () => {
  const calls=[]; let finish;
  const request=(p,o)=>{calls.push({p,o});return new Promise(resolve=>{finish=resolve;});};
  const {instance}=await render(null,request);
  assert.equal(calls.length,1);assert.equal(calls[0].p,'/api/fleet');
  await click(instance,'Refresh');assert.equal(calls.length,1);
  await act(async()=>instance.unmount());
  assert.equal(calls[0].o.signal.aborted,true);
  finish(view());await Promise.resolve();
  assert.equal(calls.length,1);
});

test('a registered agent before its first poll is visible but unconfirmed and excluded from totals', async () => {
  const pending=rig('new',{snapshot:{},config:{},last_seen:0}),raw=view([pending]);
  const normalized=decodeFleetView(raw);
  assert.equal(normalized.rigs.length,1);assert.equal(normalized.rigs[0].snapshot,null);
  assert.equal(normalized.rigs[0].config,null);assert.equal(normalized.rigs[0].online,false);
  assert.equal(fleetSummary(normalized.rigs).online,0);
  const {instance,calls}=await render(raw);
  try {
    assert.match(text(instance.toJSON()),/Rig new/);assert.match(text(instance.toJSON()),/Waiting for first snapshot/);
    await click(instance,'Rig new');assert.match(text(instance.toJSON()),/has not reported a complete valid snapshot/);
    assert.equal(button(instance,'Start'),undefined);
    await click(instance,'GPU');assert.match(text(instance.toJSON()),/no verified cached GPU inventory/);
    assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
  } finally { await act(async()=>instance.unmount()); }
});

test('malformed nested snapshots never become healthy empty telemetry', () => {
  const defects=[
    s=>({...s,status:null}),s=>({...s,status:{}}),
    s=>({...s,status:[{...s.status[0],metrics:null}]}),
    s=>({...s,status:[{...s.status[0],metrics:{...s.status[0].metrics,hashrate_hps:'1000'}}]}),
    s=>({...s,status:[{...s.status[0],mode:{toString:1}}]}),
    s=>({...s,status:[{...s.status[0],uptime_seconds:'10'}]}),
    s=>({...s,status:[{...s.status[0],dev_tip_active:'yes'}]}),
    s=>({...s,status:[{...s.status[0],metrics:{...s.status[0].metrics,accepted:'1'}}]}),
    s=>({...s,gpu_inventory:[{selector:0,name:{}}]}),
  ];
  for(const defect of defects){const r=rig('broken');r.snapshot=defect(r.snapshot);const result=decodeFleetView(view([r,rig('good')]));
    assert.equal(result.rigs[0].snapshot,null);assert.equal(result.rigs[0].online,false);assert.equal(result.rigs[1].online,true);
    assert.equal(fleetSummary(result.rigs).online,1);
  }
  assert.equal(decodeFleetLogs({profile_id:'p',lines:[null]}),null);
});

test('malformed configuration disables commands and editors without crashing the rig detail', async () => {
  const broken=rig('bad-config');broken.config={revision:'x',profiles:null,schedules:[]};
  const {instance,calls}=await render(view([broken]));
  try {
    await click(instance,'Rig bad-config');assert.match(text(instance.toJSON()),/configuration is incomplete or invalid/);
    assert.equal(button(instance,'Start').props.disabled,true);assert.equal(button(instance,'Edit').props.disabled,true);
    await click(instance,'Schedules');assert.equal(button(instance,'Edit schedules').props.disabled,true);
    await click(instance,'GPU');assert.match(text(instance.toJSON()),/Stale readings/);
    assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
  } finally { await act(async()=>instance.unmount()); }
  for(const defect of [
    c=>({...c,profiles:[{...c.profiles[0],gpu_tuning:null}]}),
    c=>({...c,schedules:[{id:'bad',days:null}]}),
    c=>({...c,editable_profile_fields:null}),
  ]){const r=rig('bad');r.config=defect(r.config);const result=decodeFleetView(view([r]));assert.equal(result.rigs[0].config,null);assert.equal(result.rigs[0].online,false);}
});

test('create and join responses are decoded before becoming rendered state', async () => {
  for(const action of ['create','join']){
    let current=view([],{setup:{...view().setup,mode:'standalone'}});const calls=[];
    const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return current;
      if(p==='/api/fleet/'+action){current=view([rig('pending',{snapshot:{health:'online',status:null},config:{}})],{setup:{...view().setup,mode:action==='create'?'hub':'member'}});return current;}
      throw new Error('Unexpected '+p);
    };
    const {instance}=await render(current,request);
    try {
      const label=action==='create'?'Create fleet':'Join';
      const form=instance.root.findAllByType('form').find(f=>f.findAllByType('button').some(b=>text(b)===label));
      await act(async()=>{
        for(const input of form.findAllByType('input')){
          if(input.props.type==='url')input.props.onChange({target:{value:'https://192.168.1.10:17889'}});
          if(input.props.type==='password')input.props.onChange({target:{value:'one-time-fixture'}});
        }
      });
      await act(async()=>{form.props.onSubmit({preventDefault(){}});await Promise.resolve();});
      assert(calls.some(c=>c.p==='/api/fleet/'+action));
      assert.match(text(instance.toJSON()),/Rig pending/);assert.match(text(instance.toJSON()),/Unconfirmed state/);
      await click(instance,'Rig pending');assert.match(text(instance.toJSON()),/has not reported a complete valid snapshot/);
    } finally {await act(async()=>instance.unmount());}
  }
});

test('rig revocation is offered only to the authorized local coordinator and never for itself', async () => {
  for(const variant of [
    {mode:'hub',local:true,id:'remote',allowed:true},
    {mode:'hub',local:false,id:'remote',allowed:false},
    {mode:'member',local:true,id:'remote',allowed:false},
    {mode:'standalone',local:true,id:'remote',allowed:false},
    {mode:'hub',local:true,id:'local',allowed:false},
  ]){
    const data=view([rig(variant.id)],{local_setup_allowed:variant.local,setup:{...view().setup,mode:variant.mode}});
    const {instance}=await render(data);
    try {await click(instance,'Rig '+variant.id);assert.equal(!!button(instance,'Remove this rig…'),variant.allowed);}
    finally {await act(async()=>instance.unmount());}
  }
});

test('revocation requires confirmation, uses its dedicated POST and normalizes the returned view', async () => {
  let data=view([rig('local'),rig('remote')]);const calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;
    if(p==='/api/fleet/rigs/remote/revoke'){
      data=view([rig('local'),rig('pending',{snapshot:{},config:{},last_seen:0})]);return data;
    }
    throw new Error('Unexpected request '+p);
  };
  const {instance}=await render(data,request);
  try {
    await click(instance,'Rig remote');await click(instance,'Remove this rig…');
    assert.match(text(instance.toJSON()),/Local mining and schedules continue/);
    assert.equal(calls.filter(c=>c.p.endsWith('/revoke')).length,0);
    await click(instance,'Cancel');assert.equal(calls.filter(c=>c.p.endsWith('/revoke')).length,0);
    await click(instance,'Remove this rig…');await click(instance,'Revoke access');
    const sent=calls.filter(c=>c.p.endsWith('/revoke'));
    assert.equal(sent.length,1);assert.equal(sent[0].p,'/api/fleet/rigs/remote/revoke');assert.equal(sent[0].o.method,'POST');
    assert.equal(calls.filter(c=>c.p==='/api/fleet/commands').length,0);
    assert.equal(button(instance,'Rig remote'),undefined);assert.equal(button(instance,'Close details'),undefined);
    assert.match(text(instance.toJSON()),/Waiting for first snapshot/);assert.match(text(instance.toJSON()),/Access revoked/);
  } finally {await act(async()=>instance.unmount());}
});

test('schedule All uses actual profile IDs; empty active assignments cannot be saved', async () => {
  const r=rig('first');r.config.profiles.push(profile('second'));r.snapshot.status.push(miner('second'));
  r.config.schedules=[{id:'window',name:'Window',enabled:false,days:[1],start:'22:00',end:'06:00',miner_ids:[],post_action:'none',countdown_seconds:60,require_confirmation:true,wake_enabled:false,wake_minutes_before:5}];
  const data=view([r]),calls=[];
  const request=async(p,o)=>{calls.push({p,o});if(p==='/api/fleet')return data;if(p==='/api/fleet/commands')return [{id:'schedule-receipt',rig_id:'first',state:'queued',result:null,error:null}];throw new Error(p);};
  const {instance}=await render(data,request);
  try {
    await click(instance,'Rig first');await click(instance,'Schedules');
    assert.match(text(instance.toJSON()),/No profiles assigned · inactive/);
    await click(instance,'Edit schedules');
    const checkbox=label=>instance.root.findByProps({role:'dialog'}).findAllByType('label').find(l=>text(l)===label).findByType('input');
    assert.equal(checkbox('All profiles on this rig').props.checked,false);
    assert.match(text(instance.toJSON()),/No profiles assigned · inactive window/);
    await act(async()=>checkbox('Enabled').props.onChange({target:{checked:true}}));
    assert.equal(button(instance,'Save schedules').props.disabled,true);
    await act(async()=>checkbox('All profiles on this rig').props.onChange({target:{checked:true}}));
    assert.equal(checkbox('Profile first').props.checked,true);assert.equal(checkbox('Profile second').props.checked,true);
    await act(async()=>checkbox('Profile first').props.onChange({target:{checked:false}}));
    await act(async()=>checkbox('Profile second').props.onChange({target:{checked:false}}));
    assert.equal(button(instance,'Save schedules').props.disabled,true);
    await act(async()=>checkbox('All profiles on this rig').props.onChange({target:{checked:true}}));
    await click(instance,'Save schedules');
    const sent=JSON.parse(calls.find(c=>c.p==='/api/fleet/commands').o.body);
    assert.deepEqual(sent.operation.schedules[0].miner_ids,['first','second']);
  } finally {await act(async()=>instance.unmount());}
});

test('dialog typing and rerenders do not refocus Close or replace its key listener', async () => {
  const originalDocument=global.document,originalHTMLElement=global.HTMLElement;
  let closeFocus=0,restores=0,adds=0,removes=0,instance;
  class Element { focus(){restores++;} }
  const outside=new Element(),listeners={};
  global.HTMLElement=Element;global.document={activeElement:outside,addEventListener:(name,fn)=>{adds++;listeners[name]=fn;},removeEventListener:()=>{removes++;}};
  const data=view([rig('edit-focus')]),request=async()=>data;
  try {
    await act(async()=>{instance=create(React.createElement(Fleet,{language:'en',request}),{createNodeMock:element=>element.props.role==='dialog'?{querySelector:()=>({focus(){closeFocus++;}}),querySelectorAll:()=>[]}:null});await Promise.resolve();});
    await click(instance,'Rig edit-focus');await click(instance,'Edit');assert.equal(closeFocus,1);assert.equal(adds,1);
    const field=instance.root.findAllByType('input').find(i=>i.props.value==='Profile edit-focus');
    await act(async()=>field.props.onChange({target:{value:'Typed name'}}));
    await act(async()=>instance.update(React.createElement(Fleet,{language:'en',request})));
    assert.equal(closeFocus,1);assert.equal(adds,1);assert.equal(removes,0);
    await act(async()=>listeners.keydown({key:'Escape'}));assert.equal(removes,1);assert.equal(restores,1);
  } finally {if(instance)await act(async()=>instance.unmount());global.document=originalDocument;global.HTMLElement=originalHTMLElement;}
});
