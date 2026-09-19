// Runs production TypeScript modules, not a JS rewrite. No miner/GPU/Windows task
// is touched. The HTTP tests use only an ephemeral loopback server.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const vm = require('node:vm');
const http = require('node:http');
const ts = require('typescript');
const root = path.resolve(__dirname, '..');
function transpile(source, filename) {
  const result = ts.transpileModule(source, {
    fileName: filename, reportDiagnostics: true,
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  });
  const errors = (result.diagnostics || []).filter(d => d.category === ts.DiagnosticCategory.Error);
  assert.equal(errors.length, 0, errors.map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('\n'));
  return result.outputText;
}
function production(filename) {
  const full = path.join(root, filename);
  const mod = new Module(full, module);
  mod.paths = Module._nodeModulePaths(path.dirname(full));
  mod._compile(transpile(fs.readFileSync(full, 'utf8'), full), full);
  return mod.exports;
}
const { runMiningCommand, BulkStartError } = production('src/minerCommands.ts');
const { requestJson, ApiTimeoutError } = production('src/api.ts');
const defaults = () => ({
  action: 'start', id: 'profile-1', dirty: false, token: 'test-token',
  save: async () => { throw new Error('Save must not run'); },
  request: async () => ({ ok: true }),
});

async function serverTest(handler, run) {
  const timers = new Set();
  const later = (fn, ms) => { const timer = setTimeout(() => { timers.delete(timer); fn(); }, ms); timers.add(timer); };
  const server = http.createServer((req, res) => handler(req, res, later));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try { return await run(`http://127.0.0.1:${server.address().port}`); }
  finally {
    for (const timer of timers) clearTimeout(timer);
    await new Promise(resolve => { server.close(resolve); server.closeAllConnections(); });
  }
}
function json(res, value, status = 200) {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(value));
}

test('reproduces 0.7.20: a slow mandatory save aborts before any POST /start', async () => {
  const requests = [];
  await serverTest((req, res, later) => {
    requests.push(`${req.method} ${req.url}`);
    later(() => json(res, { ok: true }), 5500); // Original timeout is exactly 5000 ms.
  }, async base => {
    let busy = false;
    const messages = [];
    const context = {
      Headers, AbortController, fetch, window: { setTimeout, clearTimeout },
      resolveApiBase: () => base, config: { web: { desktop_api_port: 17888, token: '' }, ui: { language: 'fr' } },
      token: '', isTauri: false, localStorage: { setItem() {} },
      setBusy: v => { busy = v; }, setToken() {}, setDirty() {}, setStatuses() {},
      setStatusText: text => messages.push(text), t: key => key,
    };
    vm.createContext(context);
    const fixture = fs.readFileSync(path.join(__dirname, 'fixtures/manual-start-0.7.20.ts.txt'), 'utf8');
    vm.runInContext(transpile(fixture + '\nglobalThis.legacyStart = minerAction;', 'legacy.ts'), context);
    await context.legacyStart('profile-1', 'start');
    assert.deepEqual(requests, ['PUT /api/config']);
    assert.equal(busy, false);
    assert.ok(messages.some(text => text.startsWith('Save:')));
  });
});

test('clean Start sends POST immediately and never waits for a save', async () => {
  const calls = [];
  await runMiningCommand({ ...defaults(), request: async (url, options, token, timeout) => {
    calls.push([url, options.method, token, timeout]); return { ok: true };
  }});
  assert.deepEqual(calls, [['/api/miners/profile-1/start', 'POST', 'test-token', 30000]]);
});

test('dirty Start saves the latest changes before starting, with explicit stages', async () => {
  const events = [];
  let clock = 2200;
  await runMiningCommand({ ...defaults(), dirty: true,
    onStage: stage => events.push(stage),
    save: async () => { clock = 2400; events.push('saved-2400'); },
    request: async () => { assert.equal(clock, 2400); events.push('posted'); return { ok: true }; },
  });
  assert.deepEqual(events, ['saving', 'saved-2400', 'sending', 'posted']);
});

test('dirty Start can complete a save longer than the old five-second limit', async () => {
  const requests = [];
  await serverTest((req, res, later) => {
    requests.push(`${req.method} ${req.url}`);
    if (req.method === 'PUT') later(() => json(res, { ok: true }), 5500);
    else json(res, { ok: true });
  }, async base => {
    await runMiningCommand({ ...defaults(), dirty: true,
      save: () => requestJson(`${base}/api/config`, { method: 'PUT', body: '{}' }, '', 15000),
      request: (url, opts, token, timeout) => requestJson(base + url, opts, token, timeout),
    });
  });
  assert.deepEqual(requests, ['PUT /api/config', 'POST /api/miners/profile-1/start']);
});

test('save failure is propagated and no stale configuration is started', async () => {
  let posted = false;
  await assert.rejects(runMiningCommand({ ...defaults(), dirty: true,
    save: async () => { throw new Error('Disk write failed'); },
    request: async () => { posted = true; return { ok: true }; },
  }), /Disk write failed/);
  assert.equal(posted, false);
});

for (const action of ['stop', 'stop-all']) {
  test(`${action} never saves even with unsaved edits`, async () => {
    let requested = '';
    await runMiningCommand({ ...defaults(), action, dirty: true,
      request: async url => { requested = url; return { ok: true }; },
    });
    assert.ok(requested.endsWith('/' + action));
  });
}

test('Restart encodes the profile ID and uses the same command path', async () => {
  let requested = '';
  await runMiningCommand({ ...defaults(), id: 'profile a/b', action: 'restart',
    request: async url => { requested = url; return { ok: true }; },
  });
  assert.equal(requested, '/api/miners/profile%20a%2Fb/restart');
});

test('Start all surfaces per-profile failures even with HTTP 200', async () => {
  await assert.rejects(runMiningCommand({ ...defaults(), action: 'start-all',
    request: async () => [{ id: 'ok', result: null }, { id: 'bad', result: 'Executable not found' }],
  }), error => error instanceof BulkStartError && error.startedCount === 1 && error.failures[0].id === 'bad');
});

test('Start all counts successful results, including an empty enabled-profile list', async () => {
  assert.equal((await runMiningCommand({ ...defaults(), action: 'start-all', request: async () => [] })).startedCount, 0);
  assert.equal((await runMiningCommand({ ...defaults(), action: 'start-all',
    request: async () => [{ id: 'a', result: null }, { id: 'b', result: null }],
  })).startedCount, 2);
});

test('invalid bulk/single success responses are not reported as successful starts', async () => {
  await assert.rejects(runMiningCommand({ ...defaults(), action: 'start-all', request: async () => ({ ok: true }) }), /Invalid/);
  await assert.rejects(runMiningCommand({ ...defaults(), action: 'start-all', request: async () => [{ id: 'a' }] }), /Invalid/);
  await assert.rejects(runMiningCommand({ ...defaults(), request: async () => ({}) }), /acknowledge/);
});

test('HTTP errors preserve the backend reason and successful requests send JSON/token headers', async () => {
  await serverTest((req, res) => {
    if (req.url === '/error') json(res, { error: 'GPU already used by profile A' }, 400);
    else {
      assert.equal(req.headers['x-minerdesk-token'], 'fake-token');
      assert.equal(req.headers['content-type'], 'application/json');
      json(res, { ok: true });
    }
  }, async base => {
    await assert.rejects(requestJson(`${base}/error`), /GPU already used/);
    assert.deepEqual(await requestJson(`${base}/ok`, { method: 'PUT', body: '{}' }, 'fake-token'), { ok: true });
  });
});

test('timeout covers headers and the response body, even if caller supplies a signal', async () => {
  await serverTest((req, res, later) => {
    if (req.url === '/headers') later(() => json(res, { ok: true }), 500);
    else {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.write('{');
      later(() => res.end('"ok":true}'), 500);
    }
  }, async base => {
    await assert.rejects(requestJson(`${base}/headers`, {}, '', 60), ApiTimeoutError);
    await assert.rejects(requestJson(`${base}/body`, { signal: new AbortController().signal }, '', 60), ApiTimeoutError);
  });
});

test('explicit caller cancellation is not mislabeled a timeout', async () => {
  await serverTest((_req, res, later) => later(() => json(res, { ok: true }), 500), async base => {
    const controller = new AbortController();
    controller.abort();
    await assert.rejects(requestJson(base, { signal: controller.signal }, '', 1000),
      error => !(error instanceof ApiTimeoutError) && error.name === 'AbortError');
  });
});

test('a timed-out write is never retried automatically', async () => {
  let calls = 0;
  await serverTest((_req, res, later) => { calls++; later(() => json(res, { ok: true }), 500); }, async base => {
    await assert.rejects(requestJson(base, { method: 'POST' }, '', 60), ApiTimeoutError);
    assert.equal(calls, 1);
  });
});

test('frontend keeps command errors separate from polling and controls duplicate clicks', () => {
  const app = fs.readFileSync(path.join(root, 'src/App.tsx'), 'utf8');
  assert.ok(app.includes('role="alert"'));
  assert.ok(app.includes('commandBusyRef.current = true'));
  assert.ok(app.includes('commandBusyRef.current = false'));
  assert.ok(app.includes('s.runtime.last_error'));
  assert.ok(app.includes('runMiningCommand({'));
  assert.ok(app.includes('await api("/api/config", { method: "PUT", body: JSON.stringify(next) }, token, 15000)'));
  // Syntax validation of the actual JSX source (not a browser rendering test).
  transpile(app, 'App.tsx');
});

test('Rust save path queues OS work instead of invoking Windows tools synchronously', () => {
  const lib = fs.readFileSync(path.join(root, 'src-tauri/src/lib.rs'), 'utf8');
  const save = lib.split('fn replace_config(')[1].split('fn start_windows_config_sync(')[0];
  assert.ok(save.includes('sender.send(cfg)'));
  assert.ok(!save.includes('sync_windows_wake_tasks('));
  assert.ok(!save.includes('refresh_windows_web_firewall('));
  assert.ok(save.indexOf('self.write_config_file(&cfg)?') < save.indexOf('*current = cfg.clone()'));
  assert.ok(lib.includes('applied_schedules.as_ref() != Some(&cfg.schedules)'));
  assert.ok(lib.includes('applied_web != Some(web_key)'));
  assert.ok(lib.includes('tokio::task::spawn_blocking(move || core.replace_config(cfg))'));
});
