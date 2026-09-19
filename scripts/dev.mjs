import { spawnSync } from "node:child_process";
import process from "node:process";

function run(command, args, options = {}) {
  const r = spawnSync(command, args, { stdio: "inherit", shell: process.platform === "win32", ...options });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}

if (process.platform === "win32") {
  run("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "minerdesk-headless", "--bin", "minerdesk-backend"], { shell: false });
}

// Use npm's script shell on Windows so .cmd shims work on all supported Node versions.
run(process.platform === "win32" ? "npx.cmd" : "npx", ["tauri", "dev"], { shell: process.platform === "win32" });
