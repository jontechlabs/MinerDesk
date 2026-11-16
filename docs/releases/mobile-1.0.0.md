# MinerDesk Mobile 1.0.0

**Your rigs. In your pocket.** The first stable Android companion for MinerDesk brings your existing mining PCs and Fleet to a touch-friendly dashboard.

## Download

- **Most Android phones:** `MinerDesk-Mobile_1.0.0_android-arm64.apk`.
- **Intel Android / emulator:** `MinerDesk-Mobile_1.0.0_android-x86_64.apk`.
- **Logo and icon kit:** `MinerDesk-Mobile_1.0.0_brand.zip`, with editable SVG sources, Android adaptive/themed artwork and iOS icon sources.

Requires Android 7.0/API 24 or later and a current Android System WebView. Verify downloads using `SHA256SUMS-android.txt`. These are optimized release APKs signed with the persistent MinerDesk Mobile certificate, not debug APKs. `BUILD-INFO-android.json` records the reviewed source commit and signer fingerprint. Distribution is direct APK download; no Google Play or App Store listing is included.

## What you can do

- Monitor hashrate, power, temperature, shares and runtime on your existing rigs.
- Start and stop profiles, edit supported settings and manage weekly schedules.
- Read the miner console and open your paired Fleet from your phone.
- Connect over a trusted LAN or valid HTTPS, using the rig's Web/LAN access key.
- Use the new connected-M identity, native adaptive icons and Android themed icons.

The phone controls MinerDesk on your PCs; it does not mine on the phone. Access keys stay in memory for the current session. Only the sanitized server address and preferences are remembered. Closing the phone app leaves the rigs running.

## Installing after the preview

**If you installed the 0.1.0 debug preview, uninstall only that phone app once before installing 1.0.0.** Android requires this because the production certificate differs from the temporary preview certificate. This does not remove profiles, engines, schedules or settings on your rigs. Re-enter the server URL and Web/LAN access key after installation. Future stable releases retain this signing identity and can be installed over the stable app; the mobile app does not yet provide an automatic updater.

For an HTTPS tunnel, enter its public hostname, such as `https://minerdesk.example.com`, **without the internal port**, and enter the key separately. Use the Web/API key, not a Fleet pairing invitation. See the [mobile guide](https://github.com/jontechlabs/MinerDesk/blob/main/docs/MOBILE.md).

## iOS status

The iOS source project and brand/icon set are prepared. Existing downloadable iOS preview files target the **iOS Simulator on a Mac**, not a physical iPhone. An iPhone/TestFlight/App Store release still requires an Apple signing identity and provisioning setup.

The desktop version and Windows/Linux updater remain on their separate release track.

---

## Français

**Vos rigs, dans votre poche.** MinerDesk Mobile 1.0.0 est la première version Android stable pour suivre et piloter vos PC MinerDesk et Fleet : hashrate, puissance, température, profils, horaires et console, avec une interface tactile et une nouvelle identité visuelle.

Choisissez **`android-arm64.apk`** pour un téléphone moderne. Si vous utilisiez la préversion 0.1.0, désinstallez uniquement cette application mobile une fois avant d’installer la version finale signée. Vos rigs et leurs données restent sur les PC. Les futures versions stables conserveront cette signature.

La connexion utilise l’adresse Web/API du PC et sa clé d’accès. Pour un tunnel HTTPS, utilisez le domaine public sans le port local. La clé reste en mémoire durant la session. Le projet iOS est préparé ; une distribution installable sur iPhone attend encore la signature Apple. Cette publication contient les APK directs, les logos, les empreintes de vérification et les informations de compilation.
