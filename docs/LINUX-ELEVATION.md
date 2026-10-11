# Linux miner administrator mode

MinerDesk 0.7.33 adds an optional **Run this miner as administrator (root)** checkbox in each Linux mining profile, below GPU tuning. It is off by default. It applies to the selected miner only; leave MinerDesk itself running as your normal desktop user.

1. Open **Miners**, select the profile and configure the GPU clocks, offsets or power limit supported by your miner.
2. Enable **Run this miner as administrator (root)** and save.
3. Click **Start** and approve the operating system's administrator dialog within 120 seconds.
4. Use **Stop** or **Restart** as usual. The supervisor stops the miner's process group, including worker processes. Closing the controlling backend's private pipe also stops it after an application exit or crash.

Enable this only for a miner executable you trust: the miner and its worker processes receive root privileges. MinerDesk does not collect or save your administrator password and does not add sudoers rules, persistent polkit grants or a setuid executable.

## Desktop authorization

The Debian package, AppImage and Linux portable archive include the matching `minerdesk-linux-helper` executable. Keep the helper alongside the executables when using the portable archive. The system must provide `pkexec` and a graphical polkit authentication agent. Launch MinerDesk from the Applications menu in your logged-in desktop session. On Lubuntu the session's polkit agent normally displays the password dialog.

For AppImage, MinerDesk makes a temporary private copy of the bundled helper on the host so root does not need access to the user's FUSE mount. The copy is removed after startup or cancellation; mount permissions are not changed.

If `pkexec` is missing on Debian/Ubuntu, install it with `sudo apt install pkexec`. A missing graphical authentication agent must also be installed or enabled for your desktop environment. A terminal/SSH-only session without an agent cannot approve a start; MinerDesk reports this rather than asking for a password in its console.

Approval can be required again for a restart, a scheduled start or a voluntary-support wallet switch. This mode therefore does not guarantee unattended mining. A cancelled or expired approval never starts a miner in the background.

## GPU tuning limits

Root permissions allow the miner to request settings that require administrator access. They do not add unsupported tuning features to a miner, driver or GPU. Check the miner console if a setting is refused. NVIDIA documents administrator requirements and hardware restrictions for GPU and memory clock controls in its [nvidia-smi documentation](https://docs.nvidia.com/deploy/nvidia-smi/index.html).

The helper passes the executable and each argument directly, without shell interpretation. Profile names with spaces remain a single argument. Live output and the real miner exit code are forwarded to MinerDesk, including SRBMiner's terminal-dependent output.
