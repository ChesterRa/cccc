// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vite-plus/test";
import { useVoiceRecordingSession } from "./useVoiceRecordingSession";
import { usePendingPageWork } from "../../../stores/usePendingPageWork";

describe("voice recording session ownership", () => {
  it("does not let an old run close or alter a newer recording", async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    usePendingPageWork.setState({ owners: new Set() });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let session!: ReturnType<typeof useVoiceRecordingSession>;
    function Probe() {
      session = useVoiceRecordingSession();
      return <span>{session.phase}</span>;
    }
    const input = {
      scope: {
        groupId: "A",
        documentPath: "",
        captureMode: "prompt" as const,
        dispatchTarget: "composer" as const,
        composerText: "draft",
        composerContext: {},
      },
      config: {},
      audioPreferences: { inputDeviceId: "", outputDeviceId: "" },
    };
    try {
      await act(async () => root.render(<Probe />));
      let first!: NonNullable<ReturnType<typeof session.begin>>;
      await act(async () => {
        first = session.begin(input)!;
      });
      expect(usePendingPageWork.getState().owners.size).toBe(1);
      expect(session.begin(input)).toBeNull();
      first.browserFinalText = "recognized";
      await act(async () => {
        session.markRecording(first.id);
        session.markFinishing(first.id);
      });
      expect(session.begin(input)).toBeNull();
      let ended: ReturnType<typeof session.end> = null;
      await act(async () => {
        ended = session.end(first.id);
      });
      expect(usePendingPageWork.getState().owners.size).toBe(0);
      expect(ended).toMatchObject({ scope: { groupId: "A" }, browserFinalText: "recognized" });
      let second!: NonNullable<ReturnType<typeof session.begin>>;
      await act(async () => {
        second = session.begin(input)!;
      });
      expect(session.markRecording(first.id)).toBe(false);
      expect(session.markFinishing(first.id)).toBe(false);
      expect(session.end(first.id)).toBeNull();
      expect(session.current.current?.id).toBe(second.id);
      expect(session.current.current?.browserFinalText).toBe("");
      expect(usePendingPageWork.getState().owners.size).toBe(1);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
    expect(usePendingPageWork.getState().owners.size).toBe(0);
  });
});
