# Linux build and packaging

Target: Linux x86_64. MinerDesk uses WebKitGTK 4.1 for the desktop. The standalone CLI still links the project's shared Tauri library, so it is **not a dependency-free static executable**.

## Prerequisites

On Debian 12 or a compatible Ubuntu installation:

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

Install current Node.js LTS with npm and Rust stable from their official sources. Confirm `node --version`, `npm --version`, `rustc --version` and `cargo --version`. The [official Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are the reference for other distributions.

## Release build

```bash
npm ci --include=dev
bash scripts/build-linux.sh
```

The script validates dependencies/version metadata, builds React, runs tests, compiles the CLI, packages the desktop as `.deb` and AppImage, and copies validated current-version outputs to `publish/linux-x64/` with SHA-256 hashes. Errors stop the script immediately.

```bash
# Debian package only
MINERDESK_BUNDLES=deb bash scripts/build-linux.sh

# Direct desktop packaging after dependencies and frontend preparation
npx tauri build --bundles deb,appimage
```

The normal Tauri output is under `src-tauri/target/release/bundle/`. `CARGO_TARGET_DIR` can change that base path. The default binary names are `minerdesk` and `minerdesk-headless`. The Windows-only executable resources, NSIS options and hooks are confined to `tauri.windows.conf.json`; Linux uses `tauri.linux.conf.json`.

The `.deb` installs the desktop and standalone CLI. The compatibility `minerdesk-backend` executable is also packaged, but the Linux desktop hosts its own backend; use `minerdesk-headless` for independent operation. A separate release tarball provides the desktop and standalone CLI. No third-party mining engine is included. RPM, ARM and macOS packages are not claimed as tested.

## Runtime and headless use

```bash
sudo apt install ./MinerDesk_0.7.22_amd64.deb
minerdesk

# Installed standalone CLI (or ./minerdesk-headless from the tarball)
minerdesk-headless --listen 127.0.0.1 --port 17888
```

The desktop requires a working graphical session. Web mode defaults to localhost. Linux does not install the Windows privileged scheduled task or Windows Job Object crash guard. GPU permissions and sleep/hibernate depend on the host; scheduled Windows wake tasks do not exist on Linux.

## Development

```bash
npm ci --include=dev
npm run tauri:dev
npm run headless:run -- --listen 127.0.0.1 --port 17888
```

## Compatibility and validation

Build on an appropriately old supported distribution to avoid raising minimum glibc requirements. The publication build uses a **Debian 12 container under WSL2**, rather than the host's newer Ubuntu userspace. That is a build/runtime baseline, not proof of compatibility with every Linux distribution.

Release validation checks package metadata/content, dynamic library resolution, headless startup/API/shutdown with isolated configuration, and a desktop smoke launch in a virtual display. It does not validate GPU mining, physical suspend/resume or every desktop environment. Exact outcomes are listed in [VALIDATION.md](docs/VALIDATION.md).

AppImage generation in a container may need `APPIMAGE_EXTRACT_AND_RUN=1` because `/dev/fuse` is unavailable. Do not claim a generated image works on a distro until it is tested there.
