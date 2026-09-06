import { describe, expect, it, vi } from "vite-plus/test";
import { applyOutputDevice } from "./codexVoiceMedia";

describe("Voice output device selection", () => {
  it("restores the system default on an audio element reused by the next call", async () => {
    let sink = "";
    const setSinkId = vi.fn(async (id: string) => {
      sink = id;
    });
    const audio = { setSinkId } as unknown as HTMLAudioElement;
    await applyOutputDevice(audio, "headset");
    expect(sink).toBe("headset");
    await applyOutputDevice(audio, "");
    expect(sink).toBe("");
    expect(setSinkId.mock.calls).toEqual([["headset"], [""]]);
  });

  it("allows browsers without selection support and reports a failed selection", async () => {
    await expect(applyOutputDevice({} as HTMLAudioElement, "")).resolves.toBeUndefined();
    const audio = {
      setSinkId: async () => {
        throw new Error("device removed");
      },
    } as unknown as HTMLAudioElement;
    await expect(applyOutputDevice(audio, "headset")).rejects.toMatchObject({
      code: "speaker_unavailable",
    });
  });
});
