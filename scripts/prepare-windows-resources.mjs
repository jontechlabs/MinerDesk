// Bootstrap resources for a clean Windows Cargo build. These are NOT releases.
// build-windows.ps1 replaces all binaries and scripts before packaging.
import { mkdirSync, existsSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
if (process.platform === 'win32') {
  const directory = new URL('../src-tauri/resources/', import.meta.url);
  mkdirSync(directory, { recursive: true });
  for (const name of ['minerdesk-backend.exe', 'minerdesk-headless.exe', 'minerdesk-update-helper.exe', 'maintenance.ps1', 'maintenance-common.ps1']) {
    const path = fileURLToPath(new URL(name, directory));
    if (!existsSync(path)) writeFileSync(path, 'MinerDesk build bootstrap; not an executable.\n');
  }
}
