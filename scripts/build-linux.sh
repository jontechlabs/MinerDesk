#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || { echo 'This script targets Linux x86_64.' >&2; exit 1; }
for tool in node npm cargo pkg-config sha256sum; do
  command -v "$tool" >/dev/null || { echo "Missing prerequisite: $tool" >&2; exit 1; }
done
pkg-config --exists webkit2gtk-4.1 gtk+-3.0 openssl
node -e 'const fs=require("fs"); const p=require("./package.json"); const t=require("./src-tauri/tauri.conf.json"); const c=fs.readFileSync("src-tauri/Cargo.toml","utf8").match(/^version\s*=\s*"([^"]+)"/m)[1]; if(p.version!==t.version||p.version!==c) throw Error("Version mismatch");'
if [[ ! -d node_modules/typescript ]]; then npm ci --include=dev; fi
npm test
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
cargo build --locked --release --manifest-path src-tauri/Cargo.toml --bin minerdesk-headless
# Set MINERDESK_BUNDLES=deb to build only Debian; default also produces AppImage.
bundles="${MINERDESK_BUNDLES:-deb,appimage}"
case "$bundles" in deb|appimage|deb,appimage) ;; *) echo 'Use deb, appimage, or deb,appimage' >&2; exit 1;; esac
npx tauri build --bundles "$bundles"
target="${CARGO_TARGET_DIR:-src-tauri/target}"
version="$(node -p 'require("./package.json").version')"
destination="publish/linux-x64"
mkdir -p "$destination"
stage="$(mktemp -d publish/.linux-staging-XXXXXX)"
trap 'rm -rf -- "$stage"' EXIT
cp "$target/release/minerdesk" "$target/release/minerdesk-headless" "$stage/"
IFS=',' read -r -a formats <<< "$bundles"
for format in "${formats[@]}"; do
  extension="$format"
  [[ "$format" != appimage ]] || extension=AppImage
  mapfile -d '' packages < <(find "$target/release/bundle/$format" -maxdepth 1 -type f -name "*_${version}_*.$extension" -print0)
  [[ ${#packages[@]} -gt 0 ]] || { echo "Missing current $format package" >&2; exit 1; }
  cp "${packages[@]}" "$stage/"
done
(cd "$stage" && sha256sum -- * > SHA256SUMS.txt)
cp "$stage/"* "$destination/"
printf 'Build complete: %s\n' "$destination"
