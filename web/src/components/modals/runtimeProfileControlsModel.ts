import { RUNTIME_INFO, supportsAcpMode, type ActorProfile } from "../../types";

export function runtimeProfileRuntimeLabel(
  profile: Pick<ActorProfile, "runtime" | "runtime_mode">,
): string {
  const runtime = String(profile.runtime || "")
    .trim()
    .toLowerCase();
  const label = RUNTIME_INFO[runtime]?.label || String(profile.runtime || "").trim();
  const mode =
    runtime === "pi"
      ? "RPC"
      : runtime === "deepseek"
        ? "ACP"
        : supportsAcpMode(runtime)
          ? profile.runtime_mode === "acp"
            ? "ACP"
            : "TUI"
          : "";
  return mode ? `${label} · ${mode}` : label;
}

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
    .filter((value): value is string => typeof value === "string")
    .map(shellQuote)
    .join(" ");
}

function shellQuote(value: string): string {
  if (/^[A-Za-z0-9_./:=+,-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'\\''`)}'`;
}
