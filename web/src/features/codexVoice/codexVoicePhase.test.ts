import { describe, expect, it } from "vite-plus/test";
import type { CodexVoicePhase } from "./codexVoiceSession";
import {
  type VoiceDotState,
  voicePhaseDotState,
  voicePhaseHasLocalActivity,
} from "./codexVoicePhase";

describe("Voice phase dot", () => {
  it.each<[CodexVoicePhase, VoiceDotState]>([
    ["idle", "inactive"],
    ["preparing", "attention"],
    ["connecting", "attention"],
    ["stopping", "attention"],
    ["listening", "active"],
    ["responding", "active"],
    ["speaking", "active"],
    ["analysing", "active"],
    ["failed", "failed"],
  ])("reports %s as %s", (phase, state) => {
    expect(voicePhaseDotState(phase, false)).toBe(state);
  });

  it("reports a call this page does not own as active whatever the local phase", () => {
    expect(voicePhaseDotState("idle", true)).toBe("active");
    expect(voicePhaseDotState("failed", true)).toBe("active");
  });

  it("does not animate preparation, external calls or muted listening as local work", () => {
    for (const phase of ["idle", "preparing", "connecting", "stopping", "failed"] as const) {
      expect(
        voicePhaseHasLocalActivity(phase, {
          microphoneMuted: false,
          externalCall: false,
          playbackBlocked: false,
        }),
      ).toBe(false);
    }
    for (const phase of ["listening", "responding", "speaking", "analysing"] as const) {
      expect(
        voicePhaseHasLocalActivity(phase, {
          microphoneMuted: false,
          externalCall: false,
          playbackBlocked: false,
        }),
      ).toBe(true);
      expect(
        voicePhaseHasLocalActivity(phase, {
          microphoneMuted: false,
          externalCall: true,
          playbackBlocked: false,
        }),
      ).toBe(false);
    }
    expect(
      voicePhaseHasLocalActivity("listening", {
        microphoneMuted: true,
        externalCall: false,
        playbackBlocked: false,
      }),
    ).toBe(false);
    expect(
      voicePhaseHasLocalActivity("speaking", {
        microphoneMuted: true,
        externalCall: false,
        playbackBlocked: false,
      }),
    ).toBe(true);
    expect(
      voicePhaseHasLocalActivity("speaking", {
        microphoneMuted: false,
        externalCall: false,
        playbackBlocked: true,
      }),
    ).toBe(false);
  });
});
