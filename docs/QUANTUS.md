# Quantus (QTC) with MinerDesk

[Download MinerDesk](https://github.com/jontechlabs/MinerDesk/releases/latest) · [English step-by-step video](https://www.youtube.com/watch?v=cCzC8PqDtEI)

MinerDesk launches and monitors SRBMiner-Multi; it does not hold wallet keys or perform mining itself. Use your own **public Quantus receiving address** for normal mining. Do not paste a recovery phrase or private key into a profile.

## Example pool profile

1. Install SRBMiner-Multi with Quantus support (introduced in 3.6.4; check the engine's current release and GPU requirements).
2. Create an SRBMiner-Multi profile and select its executable.
3. Set **Algorithm** to `quantus`. For the [Kryptex pool's TCP example](https://pool.kryptex.com/qtc), use `qtc.kryptex.network:7049`.
4. Set **Wallet** to `YOUR_QTC_ADDRESS/YourRig`. Leave the separate Worker field blank for this format. Set Password to `x`.
5. Select GPUs, enable Disable CPU if appropriate, and leave secondary/merge mining off. Pool protocols and additional parameters differ; follow the selected pool's current instructions.
6. Save, start and check the console for pool authorization and accepted shares. Confirm the worker appears at your pool. A displayed local hashrate alone does not prove a payout.

## Optional developer support in 0.7.23

Quantus profiles are recognized by the `quantus` algorithm, a Quantus/QTC pool identifier, or the Quantus `qz…` receiving-address format. `poseidon2` alone is not treated as proof of a Quantus network.

Support remains **0% by default** and is selectable per profile. When enabled, MinerDesk temporarily restarts the miner with the public developer receiving address for the selected share of mining time, then returns to the saved user address. `/worker` and `.worker` suffixes and a separately configured worker are preserved. Custom engines remain excluded; Quantus support slices use a single payout address without a merged secondary payout.

Public QTC developer-support address:

```text
qznGSCymL8d9UEpQD2QUfwtp2hQMTzuGM761gfhJYoRD1N1CD
```

The address is public and receive-only in MinerDesk. No private wallet material is part of the source, installer or binaries. This is independent of third-party engine fees. Pool minimum payouts, share difficulty and short support sessions can affect when rewards become payable; no revenue is guaranteed.
