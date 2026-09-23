import { expect, it } from "vite-plus/test";
import { computeVoiceAudioLevel } from "./voiceAudioLevel";

it("maps silence to 0 and loud speech to 1", () => {
  expect(computeVoiceAudioLevel(new Float32Array())).toBe(0);
  expect(computeVoiceAudioLevel(new Float32Array(512))).toBe(0);
  expect(computeVoiceAudioLevel(new Float32Array(512).fill(0.5))).toBe(1);
  const quiet = computeVoiceAudioLevel(new Float32Array(512).fill(0.05));
  expect(quiet).toBeGreaterThan(0);
  expect(quiet).toBeLessThan(1);
});
