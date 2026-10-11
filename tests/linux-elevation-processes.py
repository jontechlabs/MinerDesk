"""Real root supervisor tests with disposable fake miners; no GPU or mining."""
import argparse, os, pathlib, select, signal, subprocess, tempfile, time, unittest
parser=argparse.ArgumentParser();parser.add_argument('--helper',required=True,type=pathlib.Path);args=parser.parse_args()
HELPER=args.helper.resolve(); VERSION=subprocess.check_output([str(HELPER),'--version'],text=True).strip().split()[-1]

def running(pid):
    try:
        return pathlib.Path(f'/proc/{pid}/stat').read_text().split(') ')[1][0] != 'Z'
    except FileNotFoundError: return False

class Supervision(unittest.TestCase):
    def setUp(self):
        self.assertEqual(os.geteuid(),0,'Run this controlled fixture suite as root (CI sudo or WSL -u root).')
        self.directory=tempfile.TemporaryDirectory(prefix='minerdesk-root-fixture-'); self.base=pathlib.Path(self.directory.name);self.processes=[];self.pids=[]
    def tearDown(self):
        for process in self.processes:
            if process.stdin and not process.stdin.closed: process.stdin.close()
            try: process.wait(timeout=7)
            except subprocess.TimeoutExpired: process.kill();process.wait()
            for stream in (process.stdout,process.stderr):
                if stream: stream.close()
        for pid in self.pids:
            if running(pid): os.kill(pid,signal.SIGKILL)
        self.directory.cleanup()
    def launch(self,body,argv=()):
        script=self.base/'fake miner.sh';script.write_text('#!/bin/sh\n'+body);script.chmod(0o755)
        command=[str(HELPER),'--application-version',VERSION,'--miner',str(script),'--',*argv]
        process=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);self.processes.append(process)
        self.assertTrue(select.select([process.stdout],[],[],5)[0],'No ready response')
        ready=process.stdout.readline().strip();self.assertTrue(ready.startswith('MINERDESK_ELEVATION_READY '),ready)
        pid=int(ready.split()[-1]);self.pids.append(pid)
        return process,pid
    def gone(self,*pids):
        deadline=time.monotonic()+4
        while any(running(pid) for pid in pids) and time.monotonic()<deadline: time.sleep(.05)
        for pid in pids:self.assertFalse(running(pid),f'Orphaned root process {pid}')
    def test_stop_kills_root_group_including_term_ignoring_worker(self):
        process,pid=self.launch(f'trap "" TERM\nsh -c \'trap "" TERM; while :; do sleep 1; done\' &\necho $! > "{self.base}/worker"\nwhile :; do sleep 1; done\n')
        deadline=time.monotonic()+3
        while not (self.base/'worker').exists() and time.monotonic()<deadline:time.sleep(.05)
        worker=int((self.base/'worker').read_text());self.pids.append(worker)
        self.assertEqual(pathlib.Path(f'/proc/{pid}/status').read_text().split('Uid:')[1].split()[1],'0')
        process.stdin.write('stop\n');process.stdin.flush();self.assertEqual(process.wait(timeout=7),0);self.gone(pid,worker)
    def test_owner_pipe_eof_stops_miner(self):
        process,pid=self.launch('while :; do sleep 1; done\n');process.stdin.close();self.assertEqual(process.wait(timeout=7),0);self.gone(pid)
    def test_arguments_with_spaces_and_live_terminal_output_preserve_exit_error(self):
        process,pid=self.launch('test -t 1 && echo "live terminal"\nprintf "%s\\n" "$1"\necho "driver failed" >&2\nexit 17\n',('Rig with spaces',))
        output=process.stdout.read();errors=process.stderr.read();self.assertEqual(process.wait(timeout=7),17)
        self.assertIn('live terminal',output);self.assertIn('Rig with spaces',output);self.assertIn('driver failed',errors);self.gone(pid)
    def test_cancelled_owner_and_wrong_version_never_launch(self):
        marker=self.base/'should-not-exist';script=self.base/'fake.sh';script.write_text(f'#!/bin/sh\ntouch "{marker}"\n');script.chmod(0o755)
        common=[str(HELPER),'--application-version',VERSION,'--miner',str(script),'--']
        run=subprocess.run(common,input=b'',capture_output=True,timeout=5);self.assertEqual(run.returncode,125);self.assertFalse(marker.exists())
        common[2]='0.0.0';run=subprocess.run(common,input=b'keep-open',capture_output=True,timeout=5);self.assertEqual(run.returncode,125);self.assertIn(b'version differs',run.stderr);self.assertFalse(marker.exists())
    def test_supervisor_signal_stops_group(self):
        process,pid=self.launch('while :; do sleep 1; done\n');process.send_signal(signal.SIGTERM);self.assertEqual(process.wait(timeout=7),0);self.gone(pid)

unittest.main(argv=['linux-elevation-processes'])
