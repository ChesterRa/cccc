import type { HeadlessStreamEvent, StreamingActivity } from "../types";

/** Parse only the structured activity kinds that the headless client already projects. */
export function parseHeadlessActivity(ev: HeadlessStreamEvent): StreamingActivity | null {
  const type = String(ev.type || "").trim();
  if (
    ![
      "headless.activity.started",
      "headless.activity.updated",
      "headless.activity.completed",
    ].includes(type)
  )
    return null;
  const data = ev.data && typeof ev.data === "object" ? ev.data : {};
  const id = typeof data.activity_id === "string" ? data.activity_id.trim() : "";
  const summary = typeof data.summary === "string" ? data.summary.trim() : "";
  if (!id || !summary) return null;
  return {
    id,
    kind: typeof data.kind === "string" ? data.kind.trim() : "thinking",
    status: type.replace("headless.activity.", ""),
    summary,
    detail: typeof data.detail === "string" ? data.detail.trim() : undefined,
    ts: typeof ev.ts === "string" ? ev.ts : new Date().toISOString(),
    raw_item_type: typeof data.raw_item_type === "string" ? data.raw_item_type.trim() : undefined,
    tool_name: typeof data.tool_name === "string" ? data.tool_name.trim() : undefined,
    server_name: typeof data.server_name === "string" ? data.server_name.trim() : undefined,
    command: typeof data.command === "string" ? data.command.trim() : undefined,
    cwd: typeof data.cwd === "string" ? data.cwd.trim() : undefined,
    file_paths: Array.isArray(data.file_paths)
      ? data.file_paths.map((item) => String(item || "").trim()).filter(Boolean)
      : undefined,
    query: typeof data.query === "string" ? data.query.trim() : undefined,
  };
}
