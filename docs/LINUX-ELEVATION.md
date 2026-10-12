# Linux miner administrator mode

MinerDesk 0.7.33 adds an optional **Run this miner as administrator (root)** checkbox in each Linux mining profile, below GPU tuning. It is off by default. It applies to the selected miner only; leave MinerDesk itself running as your normal desktop user.

1. Open **Miners**, select the profile and configure the GPU clocks, offsets or power limit supported by your miner.
2. Enable **Run this miner as administrator (root)** and save.
3. Click **Start** and approve the operating system's administrator dialog within 120 seconds.
4. Use **Stop** or **Restart** as usual. The supervisor stops the miner's process group, including worker processes. Closing the controlling backend's private pipe also stops it after an application exit or crash.

Enable this only for a miner executable you trust: the miner and its worker processes receive root privileges. MinerDesk does not collect or save your administrator password. Interactive starts do not add persistent authorization. Optional unattended approval is described below; neither mode adds a sudoers rule or a setuid executable.

## Desktop authorization

The Debian package, AppImage and Linux portable archive include the matching `minerdesk-linux-helper` and `minerdesk-approved-helper` executables. Keep both helpers alongside the executables when using the portable archive. The system must provide `pkexec` and a graphical polkit authentication agent. Launch MinerDesk from the Applications menu in your logged-in desktop session. On Lubuntu the session's polkit agent normally displays the password dialog.

For AppImage, MinerDesk makes a temporary private copy of the bundled helper on the host so root does not need access to the user's FUSE mount. The copy is removed after startup or cancellation; mount permissions are not changed.

If `pkexec` is missing on Debian/Ubuntu, install it with `sudo apt install pkexec`. A missing graphical authentication agent must also be installed or enabled for your desktop environment. A terminal/SSH-only session without an agent cannot approve a start; MinerDesk reports this rather than asking for a password in its console.

## Scheduled and automatic starts (0.7.37)

For unattended starts, enable **Run this miner as administrator (root)**, then click **Authorize scheduled starts** on the Linux PC. Review the commands and the dedicated miner folder, and click **Approve these commands with Linux**. This stops the selected profile and asks for system administrator approval once. Click **Start** to resume the current scheduled occurrence; future scheduled starts and restarts use the saved authorization.

MinerDesk stores an authorization reference, **not an administrator password**. The constrained runner accepts only a previously approved command identifier for the Linux account that enrolled it. The executable and its supporting files are copied into private root-owned storage. The runner does not accept a new executable, shell command or argument list at launch time. Its polkit action applies only to this runner; the interactive enrollment helper still needs administrator approval. No sudoers rule or setuid executable is added.

Changing a launch setting, updating/replacing the mining engine or updating MinerDesk requires approval again. Advanced arguments, scripts and symlinks are ineligible for unattended mode; use the profile's supported fields and the original ELF miner executable in a dedicated folder. Source files must belong to your account or be readable root-owned files. System drivers and libraries remain managed by the operating system. The miner itself receives full root privileges: approve only engines you trust.

Without a valid authorization, scheduled starts report **Root authorization required** and leave the profile stopped. They do not fall back to interactive password entry. Manual starts without a saved grant can still use the ordinary Linux dialog. Automatic voluntary-support switches require enrollment of both the user and support commands, shown separately during review.

**Revoke and stop this miner** removes the approval and stops its root process group. Unchecking the root option selects normal launches; use Revoke to remove the underlying authorization. A cancelled, expired or changed approval never launches a miner later after the requester closes its pipe.

A password change does not revoke existing command permissions, because there is no saved password to check. Revoke the authorization explicitly if you want to withdraw it. Authorizing/revoking is restricted to the local Linux interface; an authenticated LAN client can still start/stop an already approved profile.

Private approvals live under `/var/lib/minerdesk/root-authorization`; the constrained runner is `/usr/lib/minerdesk/minerdesk-approved-helper`, with its dedicated action in `/usr/share/polkit-1/actions/com.minerdesk.approved-miner.policy`. Debian removal/purge removes these integrations and approvals. Package upgrades retain records so they can be revoked, but the new version requires enrollment again. For portable/AppImage use, revoke profiles before deleting the application; deleting the application file alone cannot remove system integrations. Keep the matching `minerdesk-linux-helper` and `minerdesk-approved-helper` together in portable distributions.

The supervisor uses the same private owner pipe as interactive starts: Stop, revocation, owner exit or a backend crash stops the managed process group. Runtime logs may be created in the protected copy; approved executable/library bytes remain checked before subsequent launches. Polkit availability is checked without permitting interaction before an unattended launch. See the [pkexec security notes](https://polkit.pages.freedesktop.org/polkit/pkexec.1.html).

## GPU tuning limits

Root permissions allow the miner to request settings that require administrator access. They do not add unsupported tuning features to a miner, driver or GPU. Check the miner console if a setting is refused. NVIDIA documents administrator requirements and hardware restrictions for GPU and memory clock controls in its [nvidia-smi documentation](https://docs.nvidia.com/deploy/nvidia-smi/index.html).

The helper passes the executable and each argument directly, without shell interpretation. Profile names with spaces remain a single argument. Live output and the real miner exit code are forwarded to MinerDesk, including SRBMiner's terminal-dependent output.
