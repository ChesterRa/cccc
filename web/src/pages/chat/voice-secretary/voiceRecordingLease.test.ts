import { describe, expect, it } from "vite-plus/test";

import {
  voiceRecordingLeaseConflictFromDetails,
  voiceRecordingLeaseIsDefinitelyLost,
} from "./voiceRecordingLease";

describe("voiceRecordingLeaseIsDefinitelyLost", () => {
  it("keeps the local lease during transient heartbeat failures", () => {
    expect(
      voiceRecordingLeaseIsDefinitelyLost({
        ok: false,
        error: { code: "network_error", message: "temporary failure" },
      }),
    ).toBe(false);
  });

  it("recognizes explicit lease loss", () => {
    expect(
      voiceRecordingLeaseIsDefinitelyLost({
        ok: true,
        result: {
          group_id: "g1",
          action: "heartbeat",
          acquired: false,
          released: false,
          lost: true,
        },
      }),
    ).toBe(true);
    expect(
      voiceRecordingLeaseIsDefinitelyLost({
        ok: false,
        error: { code: "assistant_voice_recording_lease_lost", message: "lease lost" },
      }),
    ).toBe(true);
  });
});

describe("voiceRecordingLeaseConflictFromDetails", () => {
  it("normalizes the active recording owner and group", () => {
    expect(
      voiceRecordingLeaseConflictFromDetails({
        active_lease: {
          owner_id: " owner ",
          group_id: " original-group ",
          group_title: " Original ",
        },
      }),
    ).toEqual({ ownerId: "owner", groupId: "original-group", groupTitle: "Original" });
  });
});
