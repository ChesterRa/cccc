import type { StreamingActivity } from "../../types";
import type { LiveWorkPhase } from "./liveWorkCards";

const ACTIVE_STATUSES = new Set(["started", "waiting", "stuck"]);

function latestActivity(activities: StreamingActivity[]): StreamingActivity | undefined {
  return activities.reduce<StreamingActivity | undefined>((latest, activity) => {
    if (!latest) return activity;
    const latestTs = String(latest.ts || "");
    const activityTs = String(activity.ts || "");
    if (activityTs !== latestTs) return activityTs > latestTs ? activity : latest;
    return String(activity.id || "") > String(latest.id || "") ? activity : latest;
  }, undefined);
}

export function resolveRuntimeActivityPhase(
  basePhase: LiveWorkPhase,
  activities: StreamingActivity[],
): LiveWorkPhase {
  if (
    activities.some((activity) => ACTIVE_STATUSES.has(String(activity.status || "").toLowerCase()))
  ) {
    return "streaming";
  }
  if (basePhase === "pending" || basePhase === "streaming") return basePhase;
  const latest = latestActivity(activities);
  if (!latest) return basePhase;
  return String(latest.status || "").toLowerCase() === "failed" ? "failed" : "completed";
}
