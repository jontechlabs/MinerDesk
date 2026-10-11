# GPU tuning and SRBMiner errors

## Memory clocks and offsets (0.7.31)

Each GPU card shows the fields supported by the selected engine. Fixed core/memory clocks are absolute MHz; core/memory offsets are signed MHz added to the GPU's frequency curve. A negative offset is accepted. A fixed lock can take precedence over an offset depending on the engine. Power is watts and fan duty is percent. These are GPU controls, not CPU settings or memory-allocation limits.

| Engine | Fixed memory MHz | Core offset MHz | Memory offset MHz |
| --- | --- | --- | --- |
| SRBMiner | `--gpu-mclock0` | `--gpu-coffset0` | `--gpu-moffset0` |
| lolMiner | `--mclk` | `--coff` | `--moff` |
| BzMiner | `--oc_lock_memory_clock` | `--oc_core_clock_offset` | `--oc_memory_clock_offset` |
| Rigel | `--lock-mclock` | `--cclock` | `--mclock` |
| NPMiner | `--cuda-lock-mem-clocks` | Not exposed | Not exposed |
| lpminer | Not exposed | Not exposed | Not exposed |

SRBMiner's `0` suffix refers to the primary algorithm, not GPU 0. Lists follow the selected `--gpu-id` order. lolMiner/Rigel use indexed lists with their documented skip markers; BzMiner uses space-separated values. SRBMiner and NPMiner require complete tuning lists; BzMiner requires complete lists for the new memory locks and offsets. Use a value for each selected GPU or a separate profile. No guessed skip values are inserted for these options. An explicit zero is passed as requested: newer BzMiner versions may interpret a zero lock as an unlock, so a blank must never be silently filled with zero.

Clock tuning is generally NVIDIA-specific; support also depends on OS, mining engine version, driver and permissions. Memory locks often accept only discrete states. The memory frequency reported by a miner can use a different scale from the value accepted by the driver's clock lock; do not copy a reported figure as an automatic preset. lolMiner documents its memory-clock lock mainly for KASPA/ALPH. MinerDesk applies no clock, voltage or kernel presets. Unmapped voltage, temperature-target fan curves, OC reset/delay and algorithm-specific controls remain available through **Advanced arguments**, when supported by the engine.

These mappings are based on [SRBMiner parameters](https://github.com/doktor83/SRBMiner-Multi/blob/master/Parameters), [lolMiner's overclock options](https://github.com/Lolliedieb/lolMiner-releases), [BzMiner 23 command-line options](https://github.com/bzminer/bzminer/tree/v23.0.2), [Rigel's CLI](https://github.com/rigelminer/rigel) and [NPMiner's CUDA options](https://github.com/nushypool/npminer). The adapter emits options, but a successful GPU/driver application must be checked in your miner console.

## Units and startup errors

**GPU core clock MHz** sets the GPU core frequency. It is not the CPU frequency. **Power limit W** sets a power-management limit in watts; it is not a memory clock or memory-size limit. SRBMiner's `CClk` and `MClk` statistics report GPU core and memory clocks separately.

SRBMiner documents `--gpu-cclock0` in MHz and `--gpu-plimit0` in watts. Its power parser accepts integers from **0 to 1000**, so `--gpu-plimit0 2100` is rejected before mining starts. A reported core clock such as **2745 MHz** is a different measurement and is not subject to the power parser's maximum. This distinction applies on Windows and Linux. The parser maximum is not a recommended power setting: the hardware and driver impose their own supported range. See the [upstream parameters](https://github.com/doktor83/SRBMiner-Multi/blob/master/Parameters).

From **0.7.30**, MinerDesk checks generated SRBMiner power arguments, including Advanced arguments, before starting the process. Invalid values identify the option, watts and the accepted range. A miner that exits unsuccessfully also displays an available recent error line in the dashboard, rather than only an exit number. The full output remains in Console.

## Clearing saved tuning

Older profiles can retain global/default values below the per-GPU table. Previously, a blank GPU field silently inherited those values. The table now displays effective values as real editable inputs and keeps saved global defaults visible.

Editing a GPU row snapshots its other effective settings and disables legacy inheritance for that row. Clearing its power field therefore means no power override for that GPU; an old global value does not reappear. Old profiles that have not been edited keep their original behavior. **Clear all GPU tuning** removes both per-GPU and global core/memory-clock, core/memory-offset, power-limit and fan settings for the profile. Save before starting. It preserves GPU selection, wallets, pools and schedules.

Leaving tuning blank omits MinerDesk's tuning arguments; it does not reset driver-level settings previously applied by another tool or mining session. Remove any corresponding options you entered separately in Advanced arguments.

SRBMiner needs a complete tuning list in the same order as the selected GPU IDs. A mixture of blank and set values now produces a clear message instead of silently dropping the entire option. Clear the option for every selected card, provide every value, or use separate profiles to tune only some cards.

## Names with spaces

MinerDesk launches engines directly with an argument vector, without a shell. A rig name such as `PRL Eco` is already passed as one argument; manually adding literal quotes to its stored name is unnecessary. Version 0.7.30 quotes names and executable paths in the displayed console command. Linux uses shell quoting; Windows uses Windows process argument quoting. This improves inspection without changing the actual launch arguments. When manually testing SRBMiner, change to its executable directory first so its relative configuration files resolve correctly.
