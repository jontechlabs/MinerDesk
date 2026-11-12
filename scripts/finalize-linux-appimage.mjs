// linuxdeploy rewrites every ELF under usr/bin and usr/lib, including our
// independent helpers. Restore their tested bytes before the final SquashFS.
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdtempSync, readFileSync, renameSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

if (process.platform !== 'linux') throw new Error('AppImage finalization requires Linux.');
const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const target = resolve(process.env.CARGO_TARGET_DIR || 'src-tauri/target');
const bundle = join(target, 'release/bundle/appimage');
const appdir = join(bundle, `${config.productName}.AppDir`);
const image = join(bundle, `${config.productName}_${config.version}_amd64.AppImage`);
if (!existsSync(image)) throw new Error('Build the current AppImage before finalizing its helpers.');
for (const name of ['minerdesk-linux-helper', 'minerdesk-approved-helper', 'minerdesk-update-helper']) {
  const destination = join(appdir, 'usr/lib', config.productName, 'resources', name);
  copyFileSync(join('src-tauri/resources', name), destination);
  chmodSync(destination, 0o755);
}
const helper = join(appdir, 'usr/bin/minerdesk-update-helper');
copyFileSync('src-tauri/resources/minerdesk-update-helper', helper);
chmodSync(helper, 0o755);

// Reuse Tauri's official output plugin without rerunning ELF deployment.
const plugin = process.env.MD_APPIMAGE_OUTPUT_PLUGIN || join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'tauri/linuxdeploy-plugin-appimage.AppImage');
if (!existsSync(plugin)) throw new Error('Tauri AppImage output plugin is missing; set MD_APPIMAGE_OUTPUT_PLUGIN to its cached path.');
const stage = mkdtempSync(join(bundle, '.helpers-'));
try {
  const output = join(stage, 'MinerDesk.AppImage');
  const result = spawnSync(plugin, ['--appimage-extract-and-run', '--appdir', appdir], {
    stdio: 'inherit', env: { ...process.env, ARCH: 'x86_64', OUTPUT: output, LDAI_OUTPUT: output, APPIMAGE_EXTRACT_AND_RUN: '1' },
  });
  if (result.error || result.status !== 0 || !existsSync(output)) throw new Error('AppImage helper finalization failed.');
  chmodSync(output, 0o755);
  renameSync(output, image);
} finally {
  if (dirname(stage) !== bundle) throw new Error('Unsafe AppImage staging path.');
  rmSync(stage, { recursive: true, force: true });
}
