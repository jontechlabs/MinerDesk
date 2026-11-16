# MinerDesk Mobile

MinerDesk Mobile is a Tauri client for Android and iOS. It connects to a MinerDesk computer and reuses its dashboard, miners, schedules, console and Fleet. Mining, scheduling and administrative services continue to run on the computer. Closing the phone app does not stop a rig.

## Install the Android release

Get [MinerDesk Mobile 1.0.0](https://github.com/jontechlabs/MinerDesk/releases/tag/mobile-v1.0.0). Android 7.0/API 24 or newer and a current Android System WebView are required.

| File | Device |
| --- | --- |
| `MinerDesk-Mobile_1.0.0_android-arm64.apk` | Recommended for modern ARM64 Android phones and tablets. |
| `MinerDesk-Mobile_1.0.0_android-x86_64.apk` | Intel Android devices and x86_64 emulators. |
| `MinerDesk-Mobile_1.0.0_brand.zip` | Editable logos, adaptive/themed launcher sources, iOS artwork and brand guide. |

Verify the download against `SHA256SUMS-android.txt`, then open the matching APK and follow Android's installer. The APK is a release build signed with MinerDesk Mobile's persistent production certificate; `BUILD-INFO-android.json` records the source revision, ABI and public signing fingerprint. The public fingerprint is also committed in [signing-certificate.json](../apps/mobile/signing-certificate.json). Private signing keys and passwords are not included in downloads.

**Moving from the 0.1.0 preview:** uninstall only MinerDesk Mobile's debug preview once, then install 1.0.0 and enter the server address/key again. Android cannot update an existing app in place when its signing certificate changes. This does not remove any rigs, miner profiles, engines or schedules from their PCs. Later stable APKs retain the production identity and can replace the installed stable app. The phone app does not yet offer its own automatic updater; install subsequent official APKs over the stable installation.

Distribution is through the GitHub APK release, not Google Play or Apple's App Store. The iOS project and icon set are prepared, while the downloadable iOS preview remains **Simulator-only** until Apple device signing is configured.

## Connect your phone

1. On the computer, open MinerDesk **Settings** and enable network/web access with a strong access key. Keep that computer running and connected to the network.
2. Open MinerDesk Mobile and enter the computer's **web API address**, for example `http://192.168.1.50:17888`, and its access key from MinerDesk Settings.
3. For unencrypted HTTP on a trusted private LAN, explicitly select **Allow unencrypted HTTP on my trusted local network**. Public HTTP connections are refused. For Internet access, use HTTPS with a valid certificate or a private VPN; do not expose an unprotected API port.
4. Open Fleet after connecting to use the fleet already associated with that computer. The connected instance must support the requested Fleet operation.

Use the web API access key, **not** a one-time Fleet invitation. Fleet's pairing/transport address, commonly port `17889`, is not the mobile app's API endpoint. Do not use `localhost` or `127.0.0.1` for another computer: on a phone they refer to the phone itself. An address copied from a browser may contain `?token=...`; the client only remembers the sanitized server address.

Direct LAN access or a private VPN is recommended. A public HTTPS gateway must enforce the access key itself if it forwards traffic to the PC's loopback API: loopback requests may be trusted locally by MinerDesk. Before accepting a connection, the mobile client verifies that an intentionally wrong key is rejected and that the real key grants access. A gateway that forwards unauthenticated requests to a locally trusted API is refused. Use an origin-only address, without a URL path prefix; redirects and certificate-verification bypasses are not supported.

For a Cloudflare Tunnel or a similar HTTPS reverse proxy, use its **public HTTPS hostname without the local port**, for example `https://minerdesk.example.com`, and enter the access key separately. The tunnel can forward internally to the PC's LAN address and port, such as `http://192.168.1.50:17888`. Avoid forwarding to `localhost` unless another gateway enforces the key. A browser-login redirect from Cloudflare Access is not supported by the current native client.

The native process keeps the access key in memory for the current app session. Only the server address and UI preferences are remembered on disk. Enter the key again after a cold launch. The client does not save it in localStorage, a config file, an APK or an iOS bundle. Disconnect or switch computers to clear the session; queued confirmations must be reviewed again for the newly selected computer.

The phone UI uses bottom navigation, labeled miner cards, stacked forms and touch-sized controls. Long logs, diagnostics and configuration dialogs scroll within their sections. Desktop-only backend repair, local file browsing and local system authorization are not phone actions; perform those steps on the relevant computer. Fleet updates remain subject to each rig's existing local opt-in and platform authorization.

## Preview artifacts

The **Mobile clients and layout** GitHub Actions workflow builds:

| Artifact | Purpose |
| --- | --- |
| `minerdesk-mobile-android-preview` | Separate arm64 and x86_64 debug-signed APKs, checksums and build provenance. Install only the APK matching the device. |
| `minerdesk-mobile-ios-simulator-preview` | A zipped `.app` for the iOS Simulator on a Mac, checksums and provenance. It is not an iPhone-installable IPA. |
| `minerdesk-mobile-layout` | Browser screenshots and assertions using clearly labeled synthetic TEST DATA at phone, tablet and landscape sizes. No real rig is controlled. |

Android preview signatures are ephemeral debug signatures, **not production signing**. A later CI run can use a different debug certificate; Android may require removing the previous preview before installing it. No debug keystore or private key is uploaded. Review the source revision and checksums before using an artifact.

The workflow runs transport/security tests, the shared frontend tests and production UI builds before distribution review. It verifies APK identity, debug signature and native ABI, and checks that the iOS artifact actually targets the simulator. Browser fixtures test layout, but do not replace testing on physical Android and iPhone devices.

For a real iPhone installation, build on a Mac with Xcode and configure your Apple signing identity/provisioning profile. App Store/TestFlight distribution requires the appropriate Apple account and signing setup; no store submission or account enrollment is performed by this project. See [Tauri iOS signing](https://v2.tauri.app/distribute/sign/ios/) and [mobile prerequisites](https://v2.tauri.app/start/prerequisites/).

## Build from source

Install root npm dependencies once (`npm ci --include=dev` at the repository root). The mobile project deliberately reuses those dependencies and has its own Tauri crate; do not initialize mobile targets in the desktop `src-tauri` directory.

```sh
node --test tests/*.test.cjs
cargo test --locked --manifest-path apps/mobile/core/Cargo.toml
npm --prefix apps/mobile run build
```

For Android, install JDK 17, Android SDK platform/build-tools 36 and NDK `29.0.14206865`, set `JAVA_HOME`, `ANDROID_HOME` and `NDK_HOME`, and install the Rust targets. Then:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cd apps/mobile
npm run tauri -- icon ../../docs/brand/mobile/icon-manifest.json --output src-tauri/icons
npm run tauri -- android init --ci --skip-targets-install
npm run tauri -- icon ../../docs/brand/mobile/icon-manifest.json --output src-tauri/icons
npm run tauri -- android build --debug --apk --split-per-abi --target aarch64 --target x86_64 --ci
```

For an unsigned simulator build on a Mac with Xcode and CocoaPods:

```sh
rustup target add aarch64-apple-ios-sim
cd apps/mobile
npm run tauri -- icon ../../docs/brand/mobile/icon-manifest.json --output src-tauri/icons
npm run tauri -- ios init --ci --skip-targets-install
npm run tauri -- icon ../../docs/brand/mobile/icon-manifest.json --output src-tauri/icons
npm run tauri -- ios build --debug --target aarch64-sim --no-sign --ci
```

On an Intel Mac use Rust target `x86_64-apple-ios` and CLI target `x86_64` instead. Open the generated `.app` with the matching simulator; do not try to install it on a physical iPhone. The production mobile frontend is bundled locally, so a Vite development server is not required for these previews.

Run icon generation **again after native initialization**. Before initialization, Tauri writes fallback icons under `src-tauri/icons/android` and `icons/ios`; after initialization, it writes the real Android resources and iOS app-icon catalog. The CI verifies those initialized resources and checks the production APK's decoded launcher pixels, so a template icon cannot silently replace the mobile brand.

## Maintainer production build

The separate **Android production release** workflow runs only from `main` and requires the exact reviewed source SHA and mobile version as dispatch inputs. It uses the `mobile-release` environment, restricted to `main`, with `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` and `ANDROID_SIGNING_CERT_SHA256`. Preserve that encrypted keystore and its recovery material privately; do not generate a different key for routine releases.

The job builds optimized APKs, verifies their signing certificate, non-debuggable/non-test manifests, identities, version codes, native ABIs and alignment, and uploads only the public release files for review. It does not publish automatically. After verification, publish a `mobile-vX.Y.Z` release with **latest=false** so desktop update discovery still targets the desktop release. Do not attach an Android release to the desktop `latest.json` updater manifest. The [brand sources](brand/mobile/README.md) feed both native icon generation and the in-app mark.

## Français

MinerDesk Mobile permet de piloter vos PC depuis Android ou iOS : tableau de bord, profils de minage, horaires, console et Fleet. Le téléphone se connecte à l'**adresse web/API** du PC et à sa **clé d'accès des paramètres**, pas à une invitation Fleet ni au port d'association `17889`.

Sur votre réseau local de confiance, HTTP demande une confirmation explicite. Pour un accès Internet, utilisez HTTPS avec un certificat valide ou un VPN privé. La clé reste uniquement en mémoire pendant la session ; seule l'adresse sans secret est mémorisée. Il faut donc ressaisir la clé après un démarrage à froid.

La **[version Android 1.0.0](https://github.com/jontechlabs/MinerDesk/releases/tag/mobile-v1.0.0)** fournit des APK de production signés avec une identité durable. Choisissez `android-arm64` pour un téléphone Android moderne. Si la préversion 0.1.0 est installée, désinstallez uniquement cette application mobile une fois avant d’installer la version finale ; les profils, horaires et clés des rigs restent sur leurs ordinateurs. Les versions stables suivantes pourront remplacer l’application stable avec la même signature. Il n’y a pas encore de mise à jour automatique propre au client mobile.

Pour un tunnel HTTPS, saisissez le nom de domaine public **sans le port local**, avec la clé dans son champ séparé. Le projet iOS reçoit les mêmes logos et icônes ; le fichier iOS disponible reste une application **pour simulateur sur Mac**, pas une IPA installable sur iPhone. Une distribution sur iPhone nécessite Xcode et une signature Apple adaptée. Aucune publication sur Google Play ou l’App Store n’est effectuée.
