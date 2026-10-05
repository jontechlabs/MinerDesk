'use strict';
const { spawn } = require('node:child_process');
// Unlike a shell step, a JS action receives the artifact service's runtime
// environment. Pass it to the pinned official actions called by the sign hook.
const child = spawn('powershell.exe', ['-NoLogo', '-NoProfile', '-File', 'scripts/build-windows.ps1', '-RequireSignature'], {
  stdio: 'inherit', env: process.env
});
child.on('error', () => { console.error('Unable to start the signed Windows build.'); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
