# Code signing policy

## Current status

The SignPath Foundation application was submitted on 6 October 2026 and is awaiting review. Windows Authenticode signing remains inactive. From 0.7.24, [application updates](UPDATES.md) use a separate cryptographic signature; this does not imply a Windows publisher certificate or SignPath approval.

**MinerDesk 0.7.29 Windows downloads are unsigned with Authenticode.** The signed Windows workflow is prepared but cannot sign anything until a trusted provider has accepted the project and its account is configured. No signing subscription or certificate has been obtained, and no claim of SignPath Foundation endorsement is made.

The proposed provider is [SignPath Foundation](https://signpath.org/), subject to [their eligibility review and terms](https://signpath.org/terms). Their certificate identifies **SignPath Foundation** as the publisher. The free program requires manual approval of signing requests; compilation, submission, signature verification and packaging are automated. Acceptance of a mining orchestration application is not guaranteed.

Once accepted and the first signed build has been verified, update this status and the README/release notes with the actual first signed version and the required attribution: “Free code signing provided by SignPath.io, certificate by SignPath Foundation”. Until then, that statement describes a future requirement, not an existing service.

## Responsibility and coverage

- Author, reviewer and proposed signing approver: [jontechlabs](https://github.com/jontechlabs), the repository maintainer. External contributions require maintainer review before integration. Enable MFA on GitHub and SignPath before requesting production signing.
- Only source-built MinerDesk programs, its NSIS installer/uninstaller and its two installer maintenance scripts may be signed with the project certificate.
- The two backends and maintenance scripts are signed **before** they are embedded in the installer. Tauri's custom signing command signs the desktop, NSIS uninstaller and final installer. Each returned file must have a valid, timestamped Authenticode signature from the configured publisher.
- NSIS supplies its uninstaller as a temporary `nstXXXX.tmp` PE. The hook submits the same bytes as `MinerDesk-uninstaller.exe`, verifies the returned signature, and restores the signed bytes to NSIS's temporary path. It never executes the installer during signing.
- Third-party mining engines are downloaded separately at the user's request and are **not** signed or endorsed by MinerDesk or the Foundation. NSIS's upstream plugins retain their upstream signature state; the hook checks their copies against Tauri's toolchain cache instead of signing them as our own code.
- Signing credentials stay in a GitHub environment secret. The provider holds the certificate's private key. Wallet recovery files are unrelated and must never enter the signing system or repository.

See the [privacy policy](PRIVACY.md) for local data, miner downloads and user-configured network connections. Signing does not guarantee compatibility, profitability or the safety of external mining engines.

## Activation by the maintainer

1. Submit the project through [SignPath's application form](https://signpath.org/apply), using the candid project information in `signing/FOUNDATION-APPLICATION.md`. The maintainer must supply their name/contact address and accept the provider's terms. Disclose the optional Defender exclusion and privileged backend; do not conceal mining functionality.
2. After acceptance, configure the SignPath project with GitHub.com as a trusted build system for `jontechlabs/MinerDesk`, GitHub-hosted runners, origin verification, maintainer approvals and a production signing policy limited to the upstream `main` branch and this workflow. Install the SignPath GitHub app if required by the provider. Do not allow contributor/fork builds to request signing.
3. Import [`signing/minerdesk-windows.xml`](../signing/minerdesk-windows.xml) as the artifact configuration with slug **`minerdesk-windows`**. Have SignPath validate the XML and a sample build; product name must be `MinerDesk` and version must match the request. Require a publicly trusted production certificate, SHA-256 and trusted timestamps. A test certificate is insufficient.
4. In GitHub **Settings → Environments → windows-signing**, restrict deployments to the `main` branch. Add secret `SIGNPATH_API_TOKEN` (a submitter token, not an approver/admin token), and these environment variables:

   | Variable | Value |
   | --- | --- |
   | `SIGNPATH_ORGANIZATION_ID` | Organization ID supplied by SignPath |
   | `SIGNPATH_PROJECT_SLUG` | Actual project slug assigned in SignPath |
   | `SIGNPATH_POLICY_SLUG` | Actual production signing policy slug |
   | `WINDOWS_SIGNING_PUBLISHER` | Exact certificate simple name, normally `SignPath Foundation` |
   | `WINDOWS_SIGNING_ENABLED` | `true`, only after all previous steps are complete |

   Never send these credentials through an issue, pull request, chat attachment or application screenshot. Rotate the token if it is exposed.
5. From **Actions → Signed Windows release → Run workflow**, select **main**. First leave **Create a new draft release** unchecked. Approve the signing requests in SignPath while the workflow runs (each request waits up to 30 minutes). Four requests are normally needed: backends/helpers, desktop, uninstaller, installer. Repeated requests for already signed resources are verified and reused.
6. Confirm the signed workflow completes and review `windows-x64-signed`, `WINDOWS-BUILD-INFO.json`, `SIGNING-RECEIPTS.jsonl` and the checksums. Verify the installer and extracted binaries on a clean Windows system; install/uninstall without running a mining session. The real service integration and Windows installation experience remain unverified until this first run.
7. For publication, increment the application version consistently in the npm, Rust and Tauri metadata and locks. Build a new version rather than replacing 0.7.23. Run the workflow with draft creation enabled, add Linux assets and final release notes, then publish the reviewed release. The draft job never overwrites an existing tag or release. Update this policy and the download page to identify the first signed version.

The standard `Build and checks` workflow remains an unsigned CI build for pull requests. The signed workflow is a separate, explicit production operation, with no unsigned fallback and no pull-request trigger. Official upload/signing action sources are pinned to full commits. An internal JavaScript action preserves GitHub's artifact service credentials for the Tauri signing callback; every submitted artifact contains only the explicitly requested files, never the source/build tree.

## Windows prompts

Authenticode identifies the publisher and protects signed bytes. It does not remove Windows UAC for a per-machine installation or certify third-party engines. A newly signed application can still receive SmartScreen warnings while reputation develops. Neither EV certificates nor any workflow can promise zero warnings on every Windows system. See [Microsoft's SmartScreen guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).

If the Foundation declines the project, an eligible organization can use [Azure Artifact Signing](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) or another trusted signing provider. This repository does not yet include an Azure signing integration. Do not substitute a self-signed certificate or ask users to disable Windows protection.
