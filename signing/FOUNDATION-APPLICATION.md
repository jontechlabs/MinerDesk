# SignPath Foundation application draft

This is a factual preparation sheet, **not a submitted or approved application**. The maintainer supplies their name, contact address and accepts the provider's terms through the official form.

| Field | Proposed value |
| --- | --- |
| Project name | MinerDesk |
| Repository / homepage | https://github.com/jontechlabs/MinerDesk |
| Download page | https://github.com/jontechlabs/MinerDesk/releases/latest |
| Privacy policy | https://github.com/jontechlabs/MinerDesk/blob/main/docs/PRIVACY.md |
| License | MIT |
| Build system | GitHub Actions, GitHub-hosted Windows and Linux runners |
| Maintainer type | Individual maintainer (confirm when submitting) |
| Tagline | Your miners. Your schedule. One dashboard. |
| Discovery source | Official SignPath documentation found during assistance with Windows code signing |

**Description**

MinerDesk is an MIT-licensed desktop and headless control center for configuring, launching, scheduling and monitoring user-selected cryptocurrency mining engines. Users choose the executable, pool and public payout address; MinerDesk does not bundle mining engines, create mining sessions without user configuration or promise earnings. The project is built from its public Rust/Tauri/React source on GitHub-hosted runners. We request signing only for our own application binaries, installer/uninstaller and installer helper scripts; upstream components and separately downloaded third-party miners must not receive our project certificate.

**Reputation (candid, do not overstate)**

MinerDesk is a recently released, actively maintained project with limited adoption. Public source and release history: https://github.com/jontechlabs/MinerDesk ; automated checks: https://github.com/jontechlabs/MinerDesk/actions/workflows/ci.yml ; tutorial channel: https://www.youtube.com/@MinerDeskApp . Published tutorials and validation records show actual configuration and testing. We do not claim independent endorsements, a large user base or independently audited security. The Foundation may consider current project reputation insufficient.

**Important disclosures for eligibility review**

- The software manages local cryptocurrency mining using separately downloaded engines, some of which have proprietary licenses and their own fees. They are not bundled with or signed by MinerDesk.
- Optional developer support is selectable per profile and **0% by default**. In supported cases it temporarily changes the payout address for a small user-selected share of runtime, preserving the saved user address.
- The Windows installer is per-machine and installs a privileged backend for configured mining/GPU operations. The desktop runs without administrator privileges. LAN control is optional and grants executable-management access; localhost is the default.
- Installer prompts offer optional outbound firewall changes and a Defender exclusion for the managed-miner directory. This may affect Foundation eligibility. These options are disclosed; we are not claiming code signing exempts them or third-party miners from security review.
- Current release 0.7.23 is unsigned. The code signing policy explicitly describes the Foundation as a proposed provider, without claiming acceptance or service provision.
- The proposed signing approver is the repository owner, jontechlabs. Confirm MFA on both accounts and provider-approved roles/approval policy during onboarding.

The draft pipeline retains upstream NSIS plugin signatures and verifies all returned project signatures and timestamps before packaging. The real provider integration, artifact schema and first signed install/uninstall require validation after acceptance. Submission should ask whether these disclosed behaviors are eligible rather than assume acceptance.
