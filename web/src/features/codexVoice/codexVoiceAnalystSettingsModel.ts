import { formatRuntimeCommand } from "../../components/modals/runtimeProfileControlsModel";
import type { CodexVoiceAnalystSettings } from "../../services/api";
import type { ActorProfile } from "../../types";

export type VoiceAnalystDraftSettings = {
  runtime: string;
  command: string;
  profile_id: string;
  profile_scope: "global" | "user";
  profile_owner: string;
};

export const emptyVoiceAnalystSettings: VoiceAnalystDraftSettings = {
  runtime: "codex",
  command: "",
  profile_id: "",
  profile_scope: "global",
  profile_owner: "",
};

export const managedAnalystRuntimes = new Set(["codex", "claude", "grok", "opencode", "kilo"]);
export const analystIdentityEnvironmentKeys = new Set([
  "CODEX_HOME",
  "CLAUDE_CONFIG_DIR",
  "GROK_HOME",
  "HOME",
  "USERPROFILE",
  "XDG_DATA_HOME",
  "XDG_CONFIG_HOME",
  "OPENCODE_CONFIG",
  "OPENCODE_CONFIG_DIR",
  "OPENCODE_DB",
  "KILO_CONFIG",
  "KILO_CONFIG_DIR",
  "KILO_DB",
]);

export function defaultAnalystRuntimeCommand(runtime: string): string {
  if (runtime === "claude") return "claude";
  if (runtime === "grok") return "grok";
  if (runtime === "opencode") return "opencode";
  if (runtime === "kilo") return "kilo";
  return "codex";
}

export function normalizeVoiceAnalystSettings(
  settings?: CodexVoiceAnalystSettings,
): VoiceAnalystDraftSettings {
  return {
    runtime: String(settings?.runtime || "codex"),
    command: formatRuntimeCommand(settings?.command),
    profile_id: String(settings?.profile_id || "").trim(),
    profile_scope: settings?.profile_scope === "user" ? "user" : "global",
    profile_owner: String(settings?.profile_owner || "").trim(),
  };
}

export function bindVoiceAnalystProfile(
  settings: VoiceAnalystDraftSettings,
  profile?: ActorProfile,
): VoiceAnalystDraftSettings {
  if (!profile) {
    return { ...settings, profile_id: "", profile_scope: "global", profile_owner: "" };
  }
  return {
    ...settings,
    profile_id: String(profile.id || "").trim(),
    profile_scope: profile.scope === "user" ? "user" : "global",
    profile_owner: String(profile.owner_id || "").trim(),
  };
}

export function voiceAnalystIdentityChanged(
  current: VoiceAnalystDraftSettings,
  loaded: VoiceAnalystDraftSettings,
  mode: "custom" | "profile",
  changedEnvironmentKeys: Iterable<string>,
  hasEnvironmentChanges: boolean,
): boolean {
  if (
    current.runtime !== loaded.runtime ||
    current.profile_id !== loaded.profile_id ||
    current.profile_scope !== loaded.profile_scope ||
    current.profile_owner !== loaded.profile_owner
  ) {
    return true;
  }
  if (mode !== "custom") return false;
  if (
    current.runtime === "claude" &&
    (current.command.trim() !== loaded.command.trim() || hasEnvironmentChanges)
  ) {
    return true;
  }
  return [...changedEnvironmentKeys].some((key) => analystIdentityEnvironmentKeys.has(key));
}
