# Security

## Supported release

Use **0.7.22 or later**. Version 0.7.22 strips a client-supplied internal locality header before assigning local trust from the TCP peer address. Earlier versions allowed a remote request to assert that it was local. Do not use earlier versions' LAN mode as an authentication boundary.

## Trust model

- MinerDesk can run configured third-party executables. Access to its API is equivalent to control of mining and executable configuration; on Windows, its desktop-owned backend is privileged.
- The local machine is trusted: loopback clients can use the API without a token. This is not isolation between untrusted users/programs on a shared computer.
- Keep the default localhost binding unless remote access is needed. Use a strong token on a trusted LAN/VPN and restrict firewall access. Never expose the HTTP port directly to the public Internet.
- The existing HTTP API does not provide TLS termination. A token alone does not encrypt that traffic. Prefer a trusted VPN such as Tailscale or WireGuard. Fleet uses a separate HTTPS listener described below.
- Wallet inputs are **public receiving addresses/usernames only**. Never supply a private key, seed phrase or exchange API secret.
- Configuration, process arguments, console logs, browser URLs/history and diagnostics may contain wallet identifiers, tokens or pool credentials. Review them before sharing.
- Third-party engine binaries have their own licenses, fees and supply-chain risks. Download verification varies by upstream; MinerDesk does not claim cryptographic verification of every miner download.
- Optional Defender exclusions cover the managed-miner directory. They are not needed for source publication and do not certify third-party binaries as safe.

## Optional Fleet access

Fleet is off by default. Its separate HTTPS listener normally uses TCP 17889; optional discovery uses UDP 17990 and supplies untrusted public hints only. Agents make outgoing HTTPS requests and pin the certificate supplied through a trusted, short-lived, single-use invitation. Do not share invitation keys publicly. Across networks, use a trusted VPN or explicitly configure coordinator reachability and firewall/router rules; MinerDesk does not create UPnP mappings.

Every paired instance is a trusted controller of the group; this first version has no separate viewer/operator/admin roles. Public payout addresses and allowed profile settings are visible to those controllers. Fleet restricts remote operations instead of forwarding them into the trusted localhost API. Executable paths, Advanced arguments, pool passwords, root enrollment and local power settings stay local. Root commands require existing rig-local approval; no administrator password is stored or sent.

The protected `fleet-private` directory beside the configuration contains TLS private material, membership credentials and command receipts. Back it up privately and exclude it from reports and source archives. Revoking a rig removes its coordinator access. A coordinator outage leaves each running backend's local mining and schedules active; Fleet does not install an automatic boot service. Log filtering and the constrained API do not constitute a full security audit of third-party engines or arbitrary console output. See the [Fleet guide](docs/FLEET.md).

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/jontechlabs/MinerDesk/security/advisories/new). Include affected version, platform, impact and a minimal reproduction with synthetic values. Do not include a real wallet secret, access token or personal configuration. Avoid posting exploitable details in a public issue while a report is under review.

This repository's publication checks are not a comprehensive security audit. Unsigned release assets have SHA-256 checksums for integrity; they do not claim code signing or a reproducible-build attestation.
