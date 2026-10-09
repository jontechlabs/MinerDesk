# GPU selection after changing hardware

Device numbers belong to the mining engine, rather than to Windows or MinerDesk. Refresh discovery after adding a card, changing drivers or enabling an integrated GPU. Select the card by name and PCI address, save the profile and restart its mining session.

SRBMiner's `--list-devices` uses a global list across OpenCL and CUDA. For example:

```text
GPU0 [0][0] [05:00.0] : amd_radeon_tm__graphics [gfx90c]
GPU1 [CUDA][0] [0000:01:00.0] : nvidia_geforce_rtx_5060_ti [blackwell]
```

Here, `--gpu-id 1` selects the RTX. The `[CUDA][0]` field is its CUDA-local index, not the value for `--gpu-id`. An engine that enumerates only CUDA devices can still call the same card GPU0. Do not copy IDs between engines.

Starting with 0.7.25, MinerDesk parses both this format and older SRBMiner rows. When discovery falls back to system inventory, indexed-engine adapters are shown without selectable GPU IDs; per-GPU tuning rows are hidden until verified selectors are available. Existing saved IDs and clocks remain intact. Manual IDs are available under **Manual IDs / diagnostics**, together with the captured output and discovery source.

Discovery can take tens of seconds during driver initialization. The interface allows 75 seconds; an engine diagnostic is stopped after 60 seconds. A slow device listing does not occupy an HTTP request worker, and responses from a previous profile are discarded.

If no suitable GPU is found, inspect the selected engine's own listing first. Choose a GPU that supports the algorithm before changing pool or overclock settings. See [SRBMiner's official parameters](https://github.com/doktor83/SRBMiner-Multi/blob/master/Parameters).

## Linux: card recognized by NVIDIA, but no engine IDs

Starting with 0.7.26, the picker explains why engine discovery failed and retains the exact launch/loader error under **Manual IDs / diagnostics**. A system inventory entry is not an engine device listing.

Linux SRBMiner can emit device listings and initialization errors only to terminal streams. MinerDesk 0.7.26 gives its diagnostic process separate pseudo-terminals for stdout/stderr, removes ANSI colors, and retains the normal timeout. This affects discovery only; it does not change mining arguments or choose fallback GPU IDs.

MinerDesk 0.7.27 extends terminal capture to live Linux SRBMiner sessions, including startup errors. A nonzero exit code reports failure, but the miner's console output is needed to identify its cause.

1. Check **Executable**. Choose the actual Linux miner executable with **Browse**, or use **Download from GitHub** and wait for its installed path to appear. Downloading may take more than five seconds. Save the profile after the path is filled in.
2. For SRBMiner, select `SRBMiner-MULTI`, not the parent folder or a Windows `.exe`. If you extracted it yourself, check execute permission on that file and that its filesystem permits execution.
3. Open a terminal in the miner's directory and run `./SRBMiner-MULTI --list-devices`. This command lists devices without starting a mining job. Inspect any missing-library or CUDA/OpenCL error. `nvidia-smi` working does not guarantee that all libraries needed by the selected miner are available.
4. Click **Refresh** and use IDs from that engine's own output. Do not copy a Windows profile's GPU ID to Linux or substitute NVIDIA's system index without checking the engine list.

If a listing prints GPU rows but MinerDesk still marks them unverified, share those rows and the discovery source with the maintainer. Keep wallet credentials, private configuration and LAN tokens out of public reports.

## Mining exits with code 1

For an initial test, clear **Core clock MHz**, **Power limit W** and **Fan %** in both the per-GPU row and **Default / legacy GPU tuning**, save, and restart the profile. Clearing a per-GPU value alone can still use the legacy fallback. Reapply tuning only after the miner works with default settings.

SRBMiner's `--gpu-plimit0` is in **watts**, whereas `--gpu-cclock0` is a fixed core clock in **MHz**. For example, `--gpu-plimit0 2100` requests 2,100 W, not 2,100 MHz. MinerDesk preserves these fields as entered and does not convert a mistaken power value to a clock setting. Use limits supported by the actual card; do not infer them from a clock value.

If mining still exits, read its console and run `./SRBMiner-MULTI --list-devices` from its own directory. Verify the selected GPU ID and any CUDA/OpenCL or missing-library error before changing drivers. A working `nvidia-smi` and a generic exit code alone do not identify the fault.
