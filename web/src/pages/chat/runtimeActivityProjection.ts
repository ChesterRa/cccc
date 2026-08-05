import type { RuntimeActivityEvent, StreamingActivity } from "../../types";

type Translate = (key: string, options?: Record<string, unknown>) => string;

function seconds(durationMs: number | null | undefined): number | null {
  if (!Number.isFinite(Number(durationMs))) return null;
  return Math.max(1, Math.round(Number(durationMs) / 1_000));
}

function label(event: RuntimeActivityEvent, t: Translate): string {
  const duration = seconds(event.duration_ms);
  const options = {
    defaultValue: "",
    tool: String(event.tool_name || "").trim() || t("runtimeActivity.toolFallback"),
    seconds: duration,
  };
  if (event.status === "stuck") {
    return t(`runtimeActivity.${event.kind}Stuck`, options);
  }
  if (event.kind === "tool") {
    return t(`runtimeActivity.tool${capitalize(event.status)}`, options);
  }
  if (event.kind === "turn") {
    return t(`runtimeActivity.turn${capitalize(event.status)}`, options);
  }
  if (event.kind === "subagent") {
    return t(`runtimeActivity.subagent${capitalize(event.status)}`, options);
  }
  return t(`runtimeActivity.session${capitalize(event.status)}`, options);
}

function capitalize(value: string): string {
  const normalized = String(value || "").trim();
  return normalized ? `${normalized[0]?.toUpperCase()}${normalized.slice(1)}` : "Updated";
}

export function projectRuntimeActivities(
  events: RuntimeActivityEvent[],
  t: Translate,
): StreamingActivity[] {
  return events
    .filter((event) => event.kind === "tool" && Boolean(String(event.tool_name || "").trim()))
    .map((event) => ({
      id: event.activity_id,
      kind: event.kind,
      status: event.status,
      summary: label(event, t),
      ts: event.ts,
      raw_item_type: event.event_type,
      tool_name: event.tool_name || undefined,
      duration_ms: event.duration_ms ?? undefined,
      runtime: event.runtime,
    }))
    .filter((activity) => activity.summary.trim());
}
