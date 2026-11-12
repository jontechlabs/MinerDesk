import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { resolve } from 'node:path';
if (process.platform === 'linux') {
  const release = process.argv.includes('--release');
  mkdirSync('src-tauri/resources', { recursive: true });
  for (const [crate, helpers] of [['linux-helper', ['minerdesk-linux-helper', 'minerdesk-approved-helper']], ['update-helper', ['minerdesk-update-helper']]]) {
    const result = spawnSync('cargo', ['build', '--locked', '--manifest-path', `src-tauri/${crate}/Cargo.toml`, ...(release ? ['--release'] : [])], { stdio: 'inherit' });
    if (result.status !== 0) process.exit(result.status || 1);
    const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : `src-tauri/${crate}/target`;
    for (const helper of helpers) {
      copyFileSync(`${target}/${release ? 'release' : 'debug'}/${helper}`, `src-tauri/resources/${helper}`);
      chmodSync(`src-tauri/resources/${helper}`, 0o755);
    }
  }
}
