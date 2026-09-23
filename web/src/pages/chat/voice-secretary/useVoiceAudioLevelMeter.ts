import { useCallback, useMemo, useRef } from "react";
import { computeVoiceAudioLevel } from "./voiceAudioLevel";

/**
 * The latest microphone level, kept in a ref and read per animation frame by
 * VoiceBeam, so audio frames never re-render the composer.
 */
export function useVoiceAudioLevelMeter() {
  const levelRef = useRef(0);
  const getLevel = useCallback(() => levelRef.current, []);
  const reset = useCallback(() => {
    levelRef.current = 0;
  }, []);
  const updateFromSamples = useCallback((samples: Float32Array) => {
    levelRef.current = computeVoiceAudioLevel(samples);
  }, []);
  return useMemo(
    () => ({ getLevel, reset, updateFromSamples }),
    [getLevel, reset, updateFromSamples],
  );
}
