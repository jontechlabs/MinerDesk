import type { MinerProfile } from "./types";

export function detectDevCoin(p: MinerProfile): string | null {
  const receivingAddress = (p.wallet || "").trim().split(/[/.]/, 1)[0];
  const wallet = (p.wallet || "").trim().toLowerCase();
  const algo = (p.algorithm || "").trim().toLowerCase();
  const pool = (p.pool || "").trim().toLowerCase();
  // Quantus SS58 uses network prefix 189 (qz...). Poseidon2 alone is not
  // a network identifier, so only a Quantus algorithm, pool or address matches.
  if (algo === "quantus" || pool.includes("quantus") || pool.split(/[/:.]/).includes("qtc") || /^qz[1-9A-HJ-NP-Za-km-z]{47}$/.test(receivingAddress)) return "QTC";
  if (wallet.startsWith("prl1") || algo.includes("pearl") || pool.includes("pearl")) return "PRL";
  if (wallet.startsWith("0x")) return "EVM";
  if (algo === "alph" || algo === "aleph" || algo.includes("alephium") || pool.includes("alephium")) return "ALPH";
  if (wallet.startsWith("xel:") || algo.includes("xelis") || pool.includes("xelis")) return "XELIS";
  if (wallet.startsWith("kaspa:") || algo === "kaspa" || pool.includes("kaspa")) return "KASPA";
  if (wallet.startsWith("nexa:") || algo.includes("nexa") || pool.includes("nexa")) return "NEXA";
  if (wallet.startsWith("nn") || algo.includes("nirmata") || pool.includes("nirmata")) return "NIRMATA";
  if (algo.includes("autolykos") || algo === "ergo" || pool.includes("ergo")) return "ERGO";
  if (algo.includes("randomx") || algo === "monero" || pool.includes("monero") || pool.includes("xmr")) return "MONERO";
  if (algo.includes("zelhash") || algo === "flux" || pool.includes("flux")) return "FLUX";
  return null;
}
