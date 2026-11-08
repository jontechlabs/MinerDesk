# Try MinerDesk with one profile

Use this walkthrough to check whether MinerDesk fits your existing mining workflow. Start with one engine and one GPU. You can browse the interface before starting a mining process.

[English demo](https://youtu.be/NFa6s8z84Xk) · [Démo en français](https://youtu.be/KBOYEVBF9Ic) · [Downloads](https://github.com/jontechlabs/MinerDesk/releases/latest)

## 1. Choose your download

Open the latest release and choose the Windows x64 installer or the Linux package suitable for your system. You do not need to clone the repository or build from source to use a release. The source-code archives are for development.

The published 0.7.22 binaries are unsigned. Compare the file with the release's SHA256SUMS.txt; this checks integrity, not publisher identity. Debian 12 is the tested Linux baseline. See [validation details](VALIDATION.md) and [Linux compatibility](../BUILD-LINUX.md#compatibility-and-validation).

## 2. Reuse settings you understand

Open **Miners**, select an engine and download it, or select an existing executable. Enter the algorithm, pool, public receiving address or pool username, and GPU selection. If you already have a working engine configuration, use its connection settings as your starting point. Leave optional tuning unchanged for the first check and save the profile.

For Pearl / PRL, confirm the current instructions from your chosen engine and pool. An engine adapter in MinerDesk does not guarantee support for every coin, engine version or GPU.

MinerDesk's optional developer tip defaults to 0%. Third-party engines and pools can have their own fees. No wallet seed or private key is needed.

## 3. Check one session

Press **Start** when you are ready to run the selected miner. In **Console**, check that the engine connects and inspect any errors. Where the engine and pool report accepted shares, confirm that they appear. In **Dashboard**, check the profile state and whichever metrics the engine supplies.

Confirm the profile is actually using the intended GPU. Missing metrics may reflect the engine output rather than a failed session; the console and pool reports help distinguish these cases. Dashboard power values are not a wall-meter measurement or proof of profitability.

## 4. Check that you remain in control

Press **Stop** and confirm the profile stops. If you want to use scheduling, stop the manual session first and create one short future weekly window under **Schedules**, with sleep/hibernate actions disabled for this initial check.

Keep the backend running. When the scheduled session starts, press **Stop** during its window. It should stay stopped for that dated occurrence rather than immediately restart. A new occurrence can run normally. A manual **Start** deliberately resumes mining and creates a manual session; it is not stopped merely because a schedule window ends. See the [scheduler rules](../README.md#scheduling-that-respects-manual-control).

## 5. Tell us where it helped or got in the way

[Share a setup report](https://github.com/jontechlabs/MinerDesk/issues/new?template=setup_report.yml) with:

- MinerDesk version, operating system, GPU and mining engine version.
- Whether installation, Start/Stop and scheduling worked; mark anything you did not test.
- The first confusing step or the feature that saved you effort.

These reports are community observations, not certified compatibility or performance benchmarks. Public reports should omit wallet identifiers, tokens, personal paths and credentials. For a reproducible defect, use the [bug report form](https://github.com/jontechlabs/MinerDesk/issues/new?template=bug_report.yml); security reports belong in [private vulnerability reporting](https://github.com/jontechlabs/MinerDesk/security/advisories/new).

If it proves useful in your workflow, consider starring the repository and sharing the demo with another miner.
