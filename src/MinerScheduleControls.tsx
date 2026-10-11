import type { MinerStatus } from "./types";
import { tr } from "./i18n";

export function ScheduleBadge({ status, hasSchedule, language }: { status: MinerStatus; hasSchedule: boolean; language: string }) {
  const t = (key: string) => tr(language, key);
  if (status.scheduler_paused) return <small className="scheduler-paused" title={t("schedulerPausedHelp")}>⏸ {t("schedulerPaused")}</small>;
  if (status.runtime.running) {
    if (status.runtime.started_by === "manual") return <small className="manual-mining" title={hasSchedule ? t("forceMiningHelp") : undefined}>{t(hasSchedule ? "forcedMining" : "manualMining")}</small>;
    if (status.runtime.started_by === "schedule") return <small className="scheduled">⏱ {t("scheduled")}</small>;
  }
  if (hasSchedule && status.profile.enabled && !status.runtime.running && !status.runtime.last_error) return <small className="scheduled">⏱ {t("waitingSchedule")}</small>;
  return null;
}

export default function MinerScheduleControls({ status, hasSchedule, busy, connected, language, onAction, onConfig }: {
  status: MinerStatus; hasSchedule: boolean; busy: boolean; connected: boolean; language: string;
  onAction: (action: "start" | "stop" | "restart" | "force-start") => void;
  onConfig: () => void;
}) {
  const t = (key: string) => tr(language, key);
  const disabled = busy || !connected || !status.profile.enabled;
  const forced = hasSchedule && status.runtime.running && status.runtime.started_by === "manual";
  return <div className="row-actions-inner">
    {status.runtime.running
      ? <button disabled={busy || !connected} onClick={() => onAction("stop")}>{t("stop")}</button>
      : <button disabled={disabled} title={hasSchedule ? t("scheduleStartHelp") : undefined} onClick={() => onAction("start")}>{t(hasSchedule && !status.scheduled_now ? "resumeSchedule" : "start")}</button>}
    {status.runtime.running && <button disabled={disabled} title={hasSchedule ? t("scheduleStartHelp") : undefined} onClick={() => onAction("restart")}>{t("restart")}</button>}
    {hasSchedule && (forced
      ? <button disabled={disabled} title={t("resumeScheduleHelp")} onClick={() => onAction("start")}>{t("resumeSchedule")}</button>
      : <button className="force-mining-button" disabled={disabled} title={t("forceMiningHelp")} onClick={() => onAction("force-start")}>{t("forceMining")}</button>)}
    <button onClick={onConfig}>{t("config")}</button>
  </div>;
}
