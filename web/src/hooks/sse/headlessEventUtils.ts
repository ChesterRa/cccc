import i18n from "../../i18n";

export function headlessActorKey(groupId: string, actorId: string): string {
  return `${String(groupId || "").trim()}:${String(actorId || "").trim()}`;
}

export function translateActorLabel(key: string, defaultValue: string): string {
  return String(i18n.t(`actors:${key}`, { defaultValue }));
}

export function formatHeadlessErrorMessage(error: unknown): string {
  if (typeof error === "string") return error.trim();
  if (!error || typeof error !== "object") return "";
  const value = error as { message?: unknown; code?: unknown; type?: unknown; status?: unknown };
  const message = String(value.message || "").trim();
  const code = String(value.code || value.type || value.status || "").trim();
  if (message && code && !message.includes(code)) return `${code}: ${message}`;
  return message || code;
}
