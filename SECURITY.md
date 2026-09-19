# Security

## Supported release

Use **0.7.22 or later**. Version 0.7.22 strips a client-supplied internal locality header before assigning local trust from the TCP peer address. Earlier versions allowed a remote request to assert that it was local. Do not use earlier versions' LAN mode as an authentication boundary.

## Trust model

- MinerDesk can run configured third-party executables. Access to its API is equivalent to control of mining and executable configuration; on Windows, its desktop-owned backend is privileged.
- The local machine is trusted: loopback clients can use the API without a token. This is not isolation between untrusted users/programs on a shared computer.
- Keep the default localhost binding unless remote access is needed. Use a strong token on a trusted LAN/VPN and restrict firewall access. Never expose the HTTP port directly to the public Internet.
- The server does not provide public TLS termination. A token alone does not encrypt traffic. Prefer a trusted VPN such as Tailscale or WireGuard.
- Wallet inputs are **public receiving addresses/usernames only**. Never supply a private key, seed phrase or exchange API secret.
- Configuration, process arguments, console logs, browser URLs/history and diagnostics may contain wallet identifiers, tokens or pool credentials. Review them before sharing.
- Third-party engine binaries have their own licenses, fees and supply-chain risks. Download verification varies by upstream; MinerDesk does not claim cryptographic verification of every miner download.
- Optional Defender exclusions cover the managed-miner directory. They are not needed for source publication and do not certify third-party binaries as safe.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/jontechlabs/MinerDesk/security/advisories/new). Include affected version, platform, impact and a minimal reproduction with synthetic values. Do not include a real wallet secret, access token or personal configuration. Avoid posting exploitable details in a public issue while a report is under review.

This repository's publication checks are not a comprehensive security audit. Unsigned release assets have SHA-256 checksums for integrity; they do not claim code signing or a reproducible-build attestation.
