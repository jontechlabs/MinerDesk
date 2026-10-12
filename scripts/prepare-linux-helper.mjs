import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { resolve } from 'node:path';
if (process.platform === 'linux') {
  const release = process.argv.includes('--release');
  const result = spawnSync('cargo', ['build', '--locked', '--manifest-path', 'src-tauri/linux-helper/Cargo.toml', ...(release ? ['--release'] : [])], { stdio: 'inherit' });
  if (result.status !== 0) process.exit(result.status || 1);
  const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : 'src-tauri/linux-helper/target';
  mkdirSync('src-tauri/resources', { recursive: true });
  for (const helper of ['minerdesk-linux-helper', 'minerdesk-approved-helper']) {
    copyFileSync(`${target}/${release ? 'release' : 'debug'}/${helper}`, `src-tauri/resources/${helper}`);
    chmodSync(`src-tauri/resources/${helper}`, 0o755);
  }
}
