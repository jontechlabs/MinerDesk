# MinerDesk Mobile 0.1.0 — preview

Access your MinerDesk computers and Fleet from a Tauri mobile app. Connect with the PC's web/API address and LAN access key, then monitor hashrate, power, temperature, shares and logs, manage profiles and schedules, or request supported Fleet actions.

The interface uses bottom navigation, labeled miner cards, touch-sized controls, stacked forms, safe areas and scrollable dialogs. The phone is a remote client; mining continues on your computers when you close it.

## Downloads

- **Android ARM64 preview APK:** for most current Android phones (Android 7 or newer).
- **Android x86_64 preview APK:** for compatible devices/emulators.
- **iOS Simulator preview ZIP:** a simulator application for a Mac, **not an IPA installable on a physical iPhone**. Use the included build provenance to check its simulator architecture. Real-iPhone installation requires Xcode and the appropriate Apple signing setup.

Android previews use a temporary debug certificate. A later preview can have a different certificate and require removing the older preview first. No production signing key or Apple credentials are included. This release is not submitted to Google Play, the App Store or TestFlight.

## Connect

Enable web/LAN access in MinerDesk on the PC. Enter its web address (typically port **17888**) and access key in the mobile app. A one-use Fleet invitation and Fleet's port 17889 are different credentials/endpoints.

Use a trusted LAN or VPN. Unencrypted HTTP requires explicit consent and is refused for public hosts. Public HTTPS requires a valid certificate and an authenticated gateway. The client verifies both key rejection and acceptance, refuses redirects and binds each command to the selected connection. Access keys are not persisted on the phone; reconnect after a cold launch. Only the sanitized server address and UI preferences are remembered.

Read the [mobile guide](https://github.com/jontechlabs/MinerDesk/blob/mobile-v0.1.0/docs/MOBILE.md) for setup, signing and build instructions.

Validation covers shared UI tests, isolated native transport tests, Android package/signature/ABI checks, iOS Simulator bundle checks and synthetic-data browser layouts at phone, tablet and landscape sizes. Physical-device testing remains necessary before production distribution. Existing desktop releases and their update feed are unchanged.

## Français

Cette préversion permet de piloter vos PC et Fleet depuis un mobile. Connectez-vous avec l'adresse web du PC et sa clé d'accès LAN. Les APK Android fournis sont des versions de test ; le ZIP iOS vise le simulateur sur Mac et ne s'installe pas directement sur iPhone. Une signature Apple reste nécessaire pour un vrai iPhone. La clé n'est pas enregistrée sur le téléphone et doit être ressaisie après un démarrage à froid.
