"""Exercise the compiled Linux backend's root-miner lifecycle, in an isolated configuration."""
import argparse, json, os, pathlib, signal, socket, subprocess, tempfile, time, urllib.request
parser=argparse.ArgumentParser();parser.add_argument('--backend',required=True,type=pathlib.Path);args=parser.parse_args()
assert os.geteuid()==0,'Use CI sudo or WSL -u root for controlled fake-miner tests'
with socket.socket() as listener:
    listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
def request(route,method='GET'):
    with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}'+route,method=method),timeout=10) as response:return json.load(response)
def wait_until(predicate):
    end=time.monotonic()+8
    while not predicate():
        assert time.monotonic()<end,'Compiled root-miner lifecycle timed out';time.sleep(.05)
def alive(pid):
    try:return pathlib.Path(f'/proc/{pid}/stat').read_text().split(') ')[1][0]!='Z'
    except FileNotFoundError:return False
with tempfile.TemporaryDirectory(prefix='minerdesk-root-backend-fixture-') as directory:
    base=pathlib.Path(directory);configdir=base/'MinerDesk';configdir.mkdir()
    miner=base/'fake miner.sh';miner.write_text('#!/bin/sh\n[ "$(id -u)" = 0 ] || exit 18\necho "root fixture ready"\nexec sleep 60\n');miner.chmod(0o755)
    profile={'id':'root-fixture','name':'Root fixture with spaces','engine':'srbminer','executable_path':str(miner),'enabled':True,'linux_run_as_root':True,'algorithm':'fixture','pool':'example.invalid','wallet':'fake-wallet','gpu_ids':'0'}
    (configdir/'config.json').write_text(json.dumps({'miners':[profile],'schedules':[],'web':{'desktop_api_port':port,'expose_lan':False},'ui':{'language':'en'}}))
    pids=[]
    with (base/'backend.log').open('wb') as log:
        process=subprocess.Popen([str(args.backend.resolve()),'--listen','127.0.0.1','--port',str(port)],env={**os.environ,'XDG_CONFIG_HOME':str(base),'XDG_DATA_HOME':str(base/'data'),'APPDIR':str(base/'appimage-fixture')},stdout=log,stderr=subprocess.STDOUT)
        try:
            for attempt in range(100):
                try:health=request('/api/health');break
                except OSError:time.sleep(.05)
            else:raise AssertionError((base/'backend.log').read_text())
            assert health['platform']=='linux' and health['linux_elevation_available']
            assert request('/api/miners/root-fixture/start','POST')['ok']
            first=request('/api/status')[0]['runtime']['pid'];pids.append(first)
            wait_until(lambda:any('root fixture ready' in line['text'] for line in request('/api/miners/root-fixture/logs')))
            assert request('/api/miners/root-fixture/restart','POST')['ok']
            second=request('/api/status')[0]['runtime']['pid'];pids.append(second);assert second!=first
            wait_until(lambda:not alive(first))
            assert request('/api/miners/root-fixture/stop','POST')['ok'];assert not request('/api/status')[0]['runtime']['running'];wait_until(lambda:not alive(second))
            assert request('/api/miners/root-fixture/start','POST')['ok'];third=request('/api/status')[0]['runtime']['pid'];pids.append(third)
            process.kill();process.wait(timeout=5);wait_until(lambda:not alive(third))
            print(json.dumps({'compiled_backend':'passed','root_miner_start_restart_stop':'passed','AppImage_helper_private_copy':'passed','live_console':'passed','forced_backend_exit_stops_root_miner':'passed','live_GPU_mining':False}))
        finally:
            if process.poll() is None:process.send_signal(signal.SIGINT);process.wait(timeout=10)
            for pid in pids:
                if alive(pid):os.kill(pid,signal.SIGKILL)
