import { describe, expect, it } from "vite-plus/test";

import type { ActorProfile } from "../../types";
import { bindVoiceAnalystProfile } from "./codexVoiceAnalystSettingsModel";

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
});
