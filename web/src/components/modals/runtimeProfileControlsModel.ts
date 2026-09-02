import type { ActorProfile } from "../../types";

export function runtimeProfileScopeLabel(
  profile: Pick<ActorProfile, "scope" | "owner_id">,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (String(profile.scope || "global").trim() === "user") {
    return t("profileScopeOwnedBy", { owner: String(profile.owner_id || "").trim() || "?" });
  }
  return t("profileScopeGlobal");
}

export function formatRuntimeCommand(command: unknown): string {
  if (typeof command === "string") return command.trim();
  if (!Array.isArray(command)) return "";
  return command
    .filter((value): value is string => typeof value === "string" && Boolean(value.trim()))
    .map(shellQuote)
    .join(" ");
}

function shellQuote(value: string): string {
  if (/^[A-Za-z0-9_./:=+,-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'\\''`)}'`;
}
