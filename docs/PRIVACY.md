# MinerDesk privacy policy

MinerDesk is a local desktop/headless mining orchestrator maintained by [jontechlabs](https://github.com/jontechlabs). It has no MinerDesk cloud account, telemetry SDK, advertising service or automatic usage analytics in the current source.

## Local data

Configuration and logs may store public wallet addresses, pool usernames, worker names, API tokens, executable paths, GPU settings, schedules, command lines and miner output on your machine. Treat pool credentials and LAN tokens as private. Never enter a seed phrase, private key or wallet recovery password: MinerDesk does not need them.

If you enable browser/LAN access, the backend serves the interface and API to the clients you connect. The default local binding is recommended. Remote HTTP access is not encrypted by MinerDesk; use a trusted private network/VPN. See [the security model](../SECURITY.md).

## Network activity

Starting with 0.7.24, automatic update checks contact GitHub's release API shortly after opening the desktop/web interface and every 6 hours while it remains open. You can turn these checks off or check manually in Settings. GitHub receives the IP address, request path and MinerDesk user agent; wallets, API tokens, profiles, logs and mining metrics are not sent. The backend caches checks for connected web clients. If you confirm an installation, the desktop also downloads release metadata and the signed package from GitHub's download hosts. The local check/snooze preferences are stored in this device's webview/browser storage. See [the update guide](UPDATES.md).

When you request an engine download, MinerDesk contacts GitHub's release API and the upstream release download host to obtain the chosen engine. Those services receive normal connection information such as your IP address, request path and user agent. GitHub's [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) applies to GitHub-hosted services.

When you start or schedule a miner, the third-party executable makes connections according to its own implementation and the pool/settings you configured. Pools receive the public payout address/username, worker identifiers and shares supplied by the miner. Review that engine's license, fees and privacy behavior and the pool's policy. MinerDesk cannot promise that all third-party executables limit their own network traffic.

Optional developer support is **0% by default**. If you choose a nonzero value, temporary mining sessions use the project's disclosed public receiving address on the configured pool; the saved personal payout address is preserved. The selected pool and engine process those sessions in the same manner as other mining sessions.

## Distribution and signing

GitHub distributes project sources and downloads and processes GitHub interactions under its own policy. A proposed SignPath signing integration processes only source-built release programs and the two installer helper scripts; personal configuration, logs and wallet recovery files must never be submitted. The signing service is not contacted by installed MinerDesk applications. See [SignPath's privacy policy](https://signpath.io/privacy-policy) and [our code signing policy](CODE-SIGNING.md).

The installer provides an uninstaller. User configuration and downloaded miners are preserved by maintenance unless you explicitly choose otherwise. Optional firewall and Defender changes require separate installer prompts. Signing is not authorization to change your security settings.

Questions about this policy can be raised through [the repository](https://github.com/jontechlabs/MinerDesk). Do not include credentials or recovery information in public issues; use [private vulnerability reporting](https://github.com/jontechlabs/MinerDesk/security/advisories/new) for security problems.
