#!/usr/bin/env node
/** Browser QA against the production mobile bundle and a synthetic native bridge.
 * No rig, external network, account, OS policy or miner is contacted or changed.
 * Usage: node scripts/mobile-layout.mjs --playwright /path/to/playwright/index.mjs
 *        --output /path/to/screenshots [--executable /path/to/existing/browser]
 */
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir, stat, writeFile } from 'node:fs/promises';
import { resolve, dirname, extname, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const options = {};
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i];
  if (!['--playwright', '--output', '--executable'].includes(key) || !process.argv[i + 1]) throw Error(`Unknown or incomplete argument: ${key}`);
  options[key.slice(2)] = process.argv[i + 1];
}
const output = resolve(options.output || resolve(repository, 'publish/mobile-layout'));
const distribution = resolve(repository, 'apps/mobile/dist');
await stat(resolve(distribution, 'index.html'));
await mkdir(output, { recursive: true });
const { chromium } = options.playwright ? await import(pathToFileURL(resolve(options.playwright)).href) : await import('playwright');

const nullTuning = {core_clock:null,memory_clock:null,core_offset:null,memory_offset:null,power_limit:null,fan:null};
const profiles = [
  {id:'prl-eco',name:'TEST · PRL Eco · RTX 5060 Ti',algorithm:'pearlhash',pool:'pool.example.invalid:3360'},
  {id:'qtc-night',name:'TEST · Quantus Night',algorithm:'quantus',pool:'pool.example.invalid:7049'},
].map(value => ({...value,...nullTuning,revision:`profile-${value.id}`,engine:'srbminer',executable_path:'/fixture-only/never-execute',enabled:true,
  wallet:'fixture-public-address',secondary_wallet:'',merge_secondary:false,merge_separator:'+',password:'x',worker:'TEST-DATA',gpu_ids:'0',gpu_tuning:[],api_port:21550,
  disable_cpu:true,extra_args:'',allow_gpu_overlap:false,linux_run_as_root:false,root_authorization:'not_requested',dev_tip_percent:0}));
const schedule = {id:'night',name:'TEST · Off-peak hours',enabled:true,days:[0,1,2,3,4,5,6],start:'22:00',end:'06:00',miner_ids:['qtc-night'],post_action:'none',countdown_seconds:60,require_confirmation:true,wake_enabled:false,wake_minutes_before:5};
const metrics = {hashrate_hps:86.5e12,power_w:167,temperature_c:62,fan_pct:44,accepted:842,rejected:1};
const statuses = profiles.map((profile, i) => ({profile,runtime:{id:profile.id,running:i===0,pid:i===0?1234:null,started_at:null,uptime_seconds:i===0?10452:0,started_by:i===0?'schedule':'',last_error:'',metrics:i===0?metrics:{hashrate_hps:null,power_w:null,temperature_c:null,fan_pct:null,accepted:0,rejected:0},dev_tip_active:false},scheduled_now:i===0,scheduler_paused:false,has_schedule:i!==0}));
const config = {miners:profiles,schedules:[schedule],web:{desktop_api_port:17888,expose_lan:true,token:''},ui:{language:'en',stop_backend_on_desktop_exit:false,start_with_windows:false,close_to_tray:false}};
const stamp = {local_time:new Date().toISOString(),timezone:'UTC',utc_offset_seconds:0};
const makeRig = (id, name, online, multiplier = 1) => ({rig_id:id,name,online,last_seen:Math.floor(Date.now()/1000)-(online?0:180),
  snapshot:{health:'online',name,os:id==='desk'?'windows':'linux',arch:'x86_64',version:'0.8.2',...stamp,revision:`snapshot-${id}`,
    status:statuses.map(s=>({id:s.profile.id,name:s.profile.name,engine:s.profile.engine,algorithm:s.profile.algorithm,enabled:true,gpu_ids:'0',running:s.runtime.running,mode:s.runtime.running?'schedule':'waiting',scheduled_now:s.scheduled_now,has_schedule:s.has_schedule,scheduler_paused:false,uptime_seconds:s.runtime.uptime_seconds,metrics:{...s.runtime.metrics,hashrate_hps:s.runtime.metrics.hashrate_hps===null?null:s.runtime.metrics.hashrate_hps*multiplier,power_w:s.runtime.metrics.power_w===null?null:s.runtime.metrics.power_w*multiplier},last_error:'',root_authorization:'not_requested'})),
    gpu_inventory:[{selector:'0',name:'NVIDIA GeForce RTX 5060 Ti',vendor:'NVIDIA',pci_bus:'0000:01:00.0'}],gpu_inventory_available:true,
    updates:{protocol:1,current_version:'0.8.2',available_version:null,can_install:false,enabled:true,reason:'up_to_date',root_required:false,root_enrolled:false,job:null}},
  config:{...stamp,revision:`config-${id}`,profiles,schedules:[schedule],editable_profile_fields:['name','enabled','pool','wallet','core_clock','memory_clock','power_limit','fan']}});
const fleet = {setup:{rig_id:'desk',name:'TEST · Home Fleet',mode:'hub',hub_url:'https://fleet.example.invalid:17889',fleet_id:'fixture-fleet',hub_enabled:true,tls_fingerprint:'fixture-fingerprint',error:null},
  rigs:[makeRig('desk','TEST · Office Windows',true),makeRig('linux','TEST · Garage Linux',true,0.8),makeRig('offline','TEST · Offline Rig',false)],commands:[],rollouts:[],coordinator_rig_id:'desk',local_setup_allowed:false,controller_allowed:true};
const data = {config,statuses,fleet,health:{platform:'windows',version:'0.8.2',headless:true,desktop_owned:true,miner_crash_guard:true,config_path:'fixture-only',scheduler:true,time:stamp.local_time},engines:[{id:'srbminer',name:'SRBMiner-Multi',github_repo:null,auto_download:false}]};

const types = {'.html':'text/html','.js':'text/javascript','.css':'text/css','.svg':'image/svg+xml','.png':'image/png','.ico':'image/x-icon'};
const server = createServer(async (request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname);
    const path = resolve(distribution, '.' + (pathname === '/' ? '/index.html' : pathname));
    if (!path.startsWith(distribution + sep)) throw Error('Path outside fixture');
    const body = await readFile(path);
    response.writeHead(200, {'Content-Type':types[extname(path)] || 'application/octet-stream','Cache-Control':'no-store'});response.end(body);
  } catch { response.writeHead(404);response.end('Fixture resource not found'); }
});
await new Promise((done, fail) => {server.once('error', fail);server.listen(0,'127.0.0.1',done);});
const origin = `http://127.0.0.1:${server.address().port}`;
let browser;
const reports=[];
try {
  browser=await chromium.launch({headless:true,...(options.executable?{executablePath:resolve(options.executable)}:{})});
  for (const [width,height] of [[320,740],[390,844],[768,1024],[844,390]]) {
    const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:1,isMobile:true,hasTouch:true});
    const page=await context.newPage(); page.setDefaultTimeout(15000);
    const failures=[];
    page.on('pageerror',error=>failures.push(error.message));
    await page.route('**/*',route=>{
      if(new URL(route.request().url()).origin!==origin){failures.push('Unexpected external request');return route.abort();}
      return route.continue();
    });
    await page.addInitScript(fixture=>{
      const clone=value=>JSON.parse(JSON.stringify(value));
      const session={connected:true,connectionId:'mobile-layout-fixture',address:'https://minerdesk-fixture.example',name:'TEST DATA · Office Windows',version:'0.8.2'};
      let connected=false;
      window.__fixtureCalls=[];
      window.__TAURI_INTERNALS__={invoke:async(command,args={})=>{
        window.__fixtureCalls.push({command,path:args.path,method:args.method});
        if(command==='mobile_connection_status')return connected?clone(session):{connected:false};
        if(command==='mobile_connect'){
          if(args.address!=='https://minerdesk-fixture.example'||args.key!=='fixture-key-never-use')throw Error('Only the test connection is allowed');
          connected=true;return clone(session);
        }
        if(command==='mobile_disconnect'){connected=false;return;}
        if(command!=='mobile_request'||!connected||args.connectionId!==session.connectionId)throw Error('Unexpected native operation in test fixture');
        if(args.method!=='GET')throw Error('The layout fixture never mutates a miner');
        const path=args.path.split('?')[0];
        const responses={'/api/health':fixture.health,'/api/config':fixture.config,'/api/engines':fixture.engines,'/api/status':fixture.statuses,'/api/fleet':fixture.fleet};
        if(path.endsWith('/logs'))return [{ts:1,stream:'stdout',text:'TEST DATA · synthetic miner output'}];
        if(!(path in responses))throw Error('Unimplemented fixture request: '+path);
        return clone(responses[path]);
      }};
      document.addEventListener('DOMContentLoaded',()=>{
        const badge=document.createElement('div');badge.id='fixture-badge';badge.textContent='TEST DATA · LOCAL UI FIXTURE';
        badge.style.cssText='position:fixed;top:2px;left:50%;transform:translateX(-50%);z-index:20000;max-width:95vw;white-space:nowrap;padding:2px 6px;border-radius:4px;background:#ffe29a;color:#16100a;font:700 9px/1.5 sans-serif;pointer-events:none';
        document.body.append(badge);
      });
    },data);
    const stage=async name=>{
      const layout=await page.evaluate(()=>{
        const viewport=window.innerWidth;
        const smallButtons=[...document.querySelectorAll('button')].flatMap(button=>{
          const rect=button.getBoundingClientRect();if(!rect.width||!rect.height||getComputedStyle(button).visibility==='hidden')return [];
          return rect.width<43.5||rect.height<43.5?[{name:button.getAttribute('aria-label')||button.textContent.trim(),width:rect.width,height:rect.height}]:[];
        });
        return {viewport,documentWidth:document.documentElement.scrollWidth,bodyWidth:document.body.scrollWidth,smallButtons};
      });
      assert(layout.documentWidth<=width+1&&layout.bodyWidth<=width+1,`${width}x${height}/${name}: horizontal page overflow ${JSON.stringify(layout)}`);
      assert.equal(layout.smallButtons.length,0,`${width}x${height}/${name}: touch buttons below44px: ${JSON.stringify(layout.smallButtons)}`);
      assert.deepEqual(failures,[],`${width}x${height}/${name}: browser errors`);
      const filename=`mobile-${width}x${height}-${name}.png`;
      await page.screenshot({path:resolve(output,filename),fullPage:false,animations:'disabled'});
      reports.push({viewport:{width,height},stage:name,file:filename,layout});
    };
    try {
      await page.goto(origin,{waitUntil:'networkidle'});
      await page.getByRole('button',{name:'Connect to MinerDesk',exact:true}).waitFor();
      await stage('connection');
      await page.getByLabel('MinerDesk URL',{exact:true}).fill('https://minerdesk-fixture.example');
      await page.getByLabel('Web access key',{exact:true}).fill('fixture-key-never-use');
      await page.getByRole('button',{name:'Connect to MinerDesk',exact:true}).click();
      await page.locator('.dashboard-table').waitFor();
      await page.getByText('TEST · PRL Eco · RTX 5060 Ti',{exact:true}).waitFor();
      await stage('dashboard');
      const navigation=page.getByRole('navigation',{name:'Main navigation'});
      assert.equal(await navigation.getByRole('button').count(),6);
      await navigation.getByRole('button',{name:'Fleet',exact:true}).click();
      await page.locator('.fleet-rig').first().waitFor();
      await stage('fleet');
      await page.locator('.fleet-rig').first().getByRole('button',{name:'Open',exact:true}).click();
      await page.locator('.fleet-detail').waitFor();
      await page.locator('.fleet-detail').scrollIntoViewIfNeeded();
      await stage('rig-details');
      await page.getByRole('button',{name:'Close details',exact:true}).click();
      await page.getByRole('button',{name:'Connections',exact:true}).click();
      const dialog=page.getByRole('dialog',{name:'Change connection?'});await dialog.waitFor();
      await page.keyboard.press('Tab');
      assert(await dialog.evaluate(element=>element.contains(document.activeElement)),'Dialog keyboard focus escaped');
      await stage('connection-dialog');
      await page.getByRole('button',{name:'Keep connection',exact:true}).click();
      const storage=await page.evaluate(()=>JSON.stringify(localStorage));
      assert(!storage.includes('fixture-key-never-use'),'Access key persisted in WebView storage');
      const calls=await page.evaluate(()=>window.__fixtureCalls);
      assert(calls.every(call=>call.command!=='mobile_request'||call.method==='GET'),'Unexpected server mutation');
    } finally {await context.close();}
  }
  await writeFile(resolve(output,'layout-report.json'),JSON.stringify({test_data:true,external_requests_allowed:false,reports},null,2)+'\n');
  console.log(JSON.stringify({result:'passed',test_data:true,viewports:4,screenshots:reports.length,output},null,2));
} finally {if(browser)await browser.close();await new Promise(done=>server.close(done));}
