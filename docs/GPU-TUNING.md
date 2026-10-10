# GPU tuning and SRBMiner errors

**GPU core clock MHz** sets the GPU core frequency. It is not the CPU frequency. **Power limit W** sets a power-management limit in watts; it is not a memory clock or memory-size limit. SRBMiner's `CClk` and `MClk` statistics report GPU core and memory clocks separately.

SRBMiner documents `--gpu-cclock0` in MHz and `--gpu-plimit0` in watts. Its power parser accepts integers from **0 to 1000**, so `--gpu-plimit0 2100` is rejected before mining starts. A reported core clock such as **2745 MHz** is a different measurement and is not subject to the power parser's maximum. This distinction applies on Windows and Linux. The parser maximum is not a recommended power setting: the hardware and driver impose their own supported range. See the [upstream parameters](https://github.com/doktor83/SRBMiner-Multi/blob/master/Parameters).

From **0.7.30**, MinerDesk checks generated SRBMiner power arguments, including Advanced arguments, before starting the process. Invalid values identify the option, watts and the accepted range. A miner that exits unsuccessfully also displays an available recent error line in the dashboard, rather than only an exit number. The full output remains in Console.

## Clearing saved tuning

Older profiles can retain global/default values below the per-GPU table. Previously, a blank GPU field silently inherited those values. The table now displays effective values as real editable inputs and keeps saved global defaults visible.

Editing a GPU row snapshots its other effective settings and disables legacy inheritance for that row. Clearing its power field therefore means no power override for that GPU; an old global value does not reappear. Old profiles that have not been edited keep their original behavior. **Clear all GPU tuning** removes both per-GPU and global core-clock, power-limit and fan settings for the profile. Save before starting. It preserves GPU selection, wallets, pools and schedules.

Leaving tuning blank omits MinerDesk's tuning arguments; it does not reset driver-level settings previously applied by another tool or mining session. Remove any corresponding options you entered separately in Advanced arguments.

SRBMiner needs a complete tuning list in the same order as the selected GPU IDs. A mixture of blank and set values now produces a clear message instead of silently dropping the entire option. Clear the option for every selected card, provide every value, or use separate profiles to tune only some cards.

## Names with spaces

MinerDesk launches engines directly with an argument vector, without a shell. A rig name such as `PRL Eco` is already passed as one argument; manually adding literal quotes to its stored name is unnecessary. Version 0.7.30 quotes names and executable paths in the displayed console command. Linux uses shell quoting; Windows uses Windows process argument quoting. This improves inspection without changing the actual launch arguments. When manually testing SRBMiner, change to its executable directory first so its relative configuration files resolve correctly.
