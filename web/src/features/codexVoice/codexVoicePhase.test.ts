import { describe, expect, it } from "vite-plus/test";
import type { CodexVoicePhase } from "./codexVoiceSession";
import {
  type VoiceDotState,
  voicePhaseDotClass,
  voicePhaseDotState,
  voicePhaseHasLocalActivity,
} from "./codexVoicePhase";

const ACTIVE = "bg-emerald-400 shadow-[0_0_0_4px_rgba(52,211,153,0.12)]";
const ATTENTION = "animate-pulse bg-amber-400";

describe("Voice phase dot", () => {
  it.each<[CodexVoicePhase, VoiceDotState, string]>([
    ["idle", "inactive", "bg-[var(--color-text-tertiary)]"],
    ["preparing", "attention", ATTENTION],
    ["connecting", "attention", ATTENTION],
    ["stopping", "attention", ATTENTION],
    ["listening", "active", ACTIVE],
    ["responding", "active", ACTIVE],
    ["speaking", "active", ACTIVE],
    ["analysing", "active", ACTIVE],
    ["failed", "failed", "bg-rose-500"],
  ])("reports %s as %s and keeps its classic classes", (phase, state, classes) => {
    expect(voicePhaseDotState(phase, false)).toBe(state);
    expect(voicePhaseDotClass(phase, false)).toBe(classes);
  });

  it("reports a call this page does not own as active whatever the local phase", () => {
    expect(voicePhaseDotState("idle", true)).toBe("active");
    expect(voicePhaseDotState("failed", true)).toBe("active");
    expect(voicePhaseDotClass("failed", true)).toBe(ACTIVE);
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
