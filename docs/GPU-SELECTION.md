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
