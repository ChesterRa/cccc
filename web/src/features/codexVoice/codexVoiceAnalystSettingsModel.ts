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

export const managedAnalystRuntimes = new Set(["codex", "grok", "opencode"]);
export const analystIdentityEnvironmentKeys = new Set([
  "CODEX_HOME",
  "GROK_HOME",
  "HOME",
  "USERPROFILE",
  "XDG_DATA_HOME",
  "XDG_CONFIG_HOME",
  "OPENCODE_CONFIG",
  "OPENCODE_CONFIG_DIR",
  "OPENCODE_DB",
]);

export function defaultAnalystRuntimeCommand(runtime: string): string {
  if (runtime === "grok") return "grok";
  if (runtime === "opencode") return "opencode";
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
