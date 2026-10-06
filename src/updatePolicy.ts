export const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
export const UPDATE_SNOOZE_MS = 24 * 60 * 60 * 1000;
export const UPDATE_RELEASES_URL = "https://github.com/jontechlabs/MinerDesk/releases/latest";
export type AppUpdate = {
  current_version: string; version: string | null; notes: string; release_url: string;
  can_install: boolean; reason: string;
};
export type UpdateProgress = { stage: "downloading" | "installing"; downloaded: number; total: number | null };
export function shouldCheckUpdate(enabled: boolean, lastCheck: number, now: number) {
  return enabled && (!Number.isFinite(lastCheck) || lastCheck <= 0 || now < lastCheck || now - lastCheck >= UPDATE_INTERVAL_MS);
}
export function shouldNotifyUpdate(update: AppUpdate, snoozedVersion: string, snoozedUntil: number, now: number) {
  return Boolean(update.version) && (update.version !== snoozedVersion || !Number.isFinite(snoozedUntil) || now >= snoozedUntil);
}
export function safeReleaseUrl(value: string) {
  return /^https:\/\/github\.com\/jontechlabs\/MinerDesk\/releases\/tag\/v\d+\.\d+\.\d+$/.test(value) ? value : UPDATE_RELEASES_URL;
}
