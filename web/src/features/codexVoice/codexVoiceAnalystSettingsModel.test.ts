import { describe, expect, it } from "vite-plus/test";

import type { ActorProfile } from "../../types";
import {
  analystIdentityEnvironmentKeys,
  bindVoiceAnalystProfile,
  defaultAnalystRuntimeCommand,
  managedAnalystRuntimes,
  normalizeVoiceAnalystSettings,
  voiceAnalystIdentityChanged,
} from "./codexVoiceAnalystSettingsModel";

const opencodeProfile = {
  id: "voice-opencode",
  name: "Voice OpenCode",
  scope: "global",
  owner_id: "",
  runtime: "opencode",
  runner: "pty",
  command: "opencode --model openai/gpt-5",
  submit: "enter",
  env: {},
  created_at: "2026-09-03T00:00:00Z",
  updated_at: "2026-09-03T00:00:00Z",
  revision: 1,
} as ActorProfile;

describe("Voice Analyst settings model", () => {
  it("admits Claude through the same runtime and identity helpers", () => {
    expect(managedAnalystRuntimes.has("claude")).toBe(true);
    expect(defaultAnalystRuntimeCommand("claude")).toBe("claude");
    expect(analystIdentityEnvironmentKeys.has("CLAUDE_CONFIG_DIR")).toBe(true);
  });

  it("preserves the complete custom runtime draft while a Runtime Profile is selected", () => {
    const custom = {
      runtime: "codex",
      command: "codex -m gpt-5.6",
      profile_id: "",
      profile_scope: "global" as const,
      profile_owner: "",
    };

    const profiled = bindVoiceAnalystProfile(custom, opencodeProfile);
    expect(profiled).toEqual({ ...custom, profile_id: "voice-opencode" });
    expect(bindVoiceAnalystProfile(profiled)).toEqual(custom);
  });

  it("treats every effective Claude command or private-environment edit as an identity change", () => {
    const loaded = {
      runtime: "claude",
      command: "claude --model sonnet",
      profile_id: "",
      profile_scope: "global" as const,
      profile_owner: "",
    };

    expect(
      voiceAnalystIdentityChanged(
        { ...loaded, command: "claude --model opus" },
        loaded,
        "custom",
        [],
        false,
      ),
    ).toBe(true);
    expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", ["ANTHROPIC_API_KEY"], true)).toBe(
      true,
    );
    expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", [], false)).toBe(false);
  });

  it("keeps Antigravity Voice on ACP and invalidates a changed model", () => {
    expect(managedAnalystRuntimes.has("antigravity")).toBe(true);
    expect(defaultAnalystRuntimeCommand("antigravity")).toBe("agy --dangerously-skip-permissions");
    const loaded = normalizeVoiceAnalystSettings({ runtime: "antigravity", command: ["agy"] });
    expect(loaded.runtime_mode).toBe("acp");
    expect(loaded.command).toBe("agy");
    expect(normalizeVoiceAnalystSettings({ runtime: "antigravity", command: [] }).command).toBe("");
    expect(
      voiceAnalystIdentityChanged(
        { ...loaded, command: "agy --model=other" },
        loaded,
        "custom",
        [],
        false,
      ),
    ).toBe(true);
    expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", [], false)).toBe(false);
  });

  it.each(["copilot", "devin", "cursor"])(
    "selects ACP and detects launch identity changes for %s Voice Analyst",
    (runtime) => {
      expect(managedAnalystRuntimes.has(runtime)).toBe(true);
      const loaded = normalizeVoiceAnalystSettings({ runtime, command: [] });
      expect(loaded.runtime_mode).toBe("acp");
      expect(loaded.command).toBe("");
      expect(defaultAnalystRuntimeCommand(runtime)).toContain(
        runtime === "cursor" ? "cursor-agent" : runtime,
      );
      expect(
        voiceAnalystIdentityChanged({ ...loaded, command: "changed" }, loaded, "custom", [], false),
      ).toBe(true);
      expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", [], false)).toBe(false);
    },
  );
  it("keeps the narrower storage-identity boundary for non-Claude custom runtimes", () => {
    const loaded = {
      runtime: "opencode",
      command: "opencode --model openai/gpt-5",
      profile_id: "",
      profile_scope: "global" as const,
      profile_owner: "",
    };

    expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", ["OPENAI_API_KEY"], true)).toBe(
      false,
    );
    expect(voiceAnalystIdentityChanged(loaded, loaded, "custom", ["OPENCODE_DB"], true)).toBe(true);
  });
});
