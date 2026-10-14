# OneZeroMiner with MinerDesk

MinerDesk 0.7.38 adds **OneZeroMiner** to the engine selector, dashboard filters and supported-engine list. Download the standalone Windows or Linux package from its [official GitHub releases](https://github.com/OneZeroMiner/onezerominer/releases) using **Download from GitHub**, or browse to an existing `onezerominer.exe` / `onezerominer` executable. The HiveOS/mmpOS custom package is not selected.

1. Create a profile and choose **OneZeroMiner**.
2. Download the engine, or choose its executable.
3. Use `pearlhash` for Pearl (PRL), or `quantus` for Quantus (QTC). Enter your pool, public receiving address, pool password and optional worker. `pearl` is accepted as an alias for `pearlhash`. Other algorithms depend on the engine version and GPU: consult its [official README](https://github.com/OneZeroMiner/onezerominer).
4. Start with **All GPUs** and blank tuning fields. OneZeroMiner has no documented standalone device-list command. System adapters are informational; if you need a specific card, enter the numeric IDs confirmed in OneZeroMiner's own console under **Manual IDs / diagnostics**. Do not reuse SRBMiner IDs without checking.
5. Set supported **Shared GPU settings** if needed. Core/memory locks use MHz, offsets signed MHz, power watts and fan percent. MinerDesk emits `--cclk`, `--mclk`, `--coff`, `--moff`, `--pl` and `--fan`. These are shared values for the active GPUs; different imported per-GPU values are rejected with an explanation rather than applied to an uncertain device order. Clear old GPU tuning before configuring shared defaults. No clock or power preset is applied automatically.
6. Save, start and inspect the console for pool authorization, accepted shares and any tuning error. Administrator permissions, drivers and hardware limit which tuning controls work. The existing [Linux root-mode guide](LINUX-ELEVATION.md) also applies to OneZeroMiner, including reviewed authorization for unattended schedules.

The adapter passes worker names as one argument, even with spaces. **Disable CPU mining** does not add an unsupported flag: OneZeroMiner is a GPU engine. If an API port is set, it binds to `127.0.0.1`; without one, MinerDesk passes `--api-disable` to avoid collisions on the engine's default port. The dashboard uses console metrics, as it does for the other engines. MinerDesk disables OneZeroMiner's optional telemetry with `--disable-telemetry`.

OneZeroMiner's own developer fee is independent of optional MinerDesk voluntary support. Existing PRL/QTC support behavior preserves your pool and worker; it is 0% by default. The secondary-wallet field is for merged payout formatting, not a second OneZeroMiner algorithm. Dual mining remains an advanced command configuration, not a separate guided adapter in this release.

## Advanced options

The engine's [1.8.0 release notes](https://github.com/OneZeroMiner/onezerominer/releases/tag/v1.8.0) describe Pearl's compact kernel for a memory clock locked at 810 MHz on supported Ampere/Ada/Blackwell cards. `--kernel 2` can be entered in **Advanced arguments** when appropriate; it is never added automatically. Quantus intensity and other algorithm-specific flags can also be entered following the current engine documentation. Advanced arguments are excluded from unattended root approval, as in other engines.

Validation covers download asset/executable selection, argument boundaries, blank/cleared tuning, invalid/conflicting GPU settings, PRL/QTC support commands and supervised fixture starts/stops. The official 1.8.1 Windows/Linux binaries were inspected, but no compatible GPU was available for a live mining benchmark.
