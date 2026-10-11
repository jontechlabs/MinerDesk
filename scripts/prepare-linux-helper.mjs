import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { resolve } from 'node:path';
if (process.platform === 'linux') {
  const release = process.argv.includes('--release');
  const result = spawnSync('cargo', ['build', '--locked', '--manifest-path', 'src-tauri/linux-helper/Cargo.toml', ...(release ? ['--release'] : [])], { stdio: 'inherit' });
  if (result.status !== 0) process.exit(result.status || 1);
  const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : 'src-tauri/linux-helper/target';
  mkdirSync('src-tauri/resources', { recursive: true });
  copyFileSync(`${target}/${release ? 'release' : 'debug'}/minerdesk-linux-helper`, 'src-tauri/resources/minerdesk-linux-helper');
  chmodSync('src-tauri/resources/minerdesk-linux-helper', 0o755);
}
