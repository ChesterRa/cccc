import { useCallback, useMemo, useRef, useState } from "react";
import {
  computeVoiceAudioLevels,
  createSilentVoiceAudioLevels,
  smoothVoiceAudioLevels,
} from "./voiceAudioLevel";

type BrowserAudioContextConstructor = typeof AudioContext;

type BrowserWindowWithAudioContext = Window & {
  webkitAudioContext?: BrowserAudioContextConstructor;
};

type VoiceAudioLevelMeterHandle = {
  stop: () => void;
};

function closeAudioContext(audioContext: AudioContext | null): void {
  if (!audioContext || audioContext.state === "closed") return;
  void audioContext.close().catch(() => undefined);
}

export function useVoiceAudioLevelMeter() {
  const levelsRef = useRef(createSilentVoiceAudioLevels());
  const browserMeterRef = useRef<VoiceAudioLevelMeterHandle | null>(null);
  const [levels, setLevels] = useState(() => createSilentVoiceAudioLevels());

  const reset = useCallback(() => {
    const silent = createSilentVoiceAudioLevels();
    levelsRef.current = silent;
    setLevels(silent);
  }, []);

  const updateFromLevels = useCallback((nextLevels: number[]) => {
    const next = smoothVoiceAudioLevels(levelsRef.current, nextLevels);
    levelsRef.current = next;
    setLevels(next);
  }, []);

  const updateFromSamples = useCallback((samples: Float32Array) => {
    updateFromLevels(computeVoiceAudioLevels(samples));
  }, [updateFromLevels]);

  const stopBrowserMeter = useCallback(() => {
    const meter = browserMeterRef.current;
    browserMeterRef.current = null;
    if (meter) meter.stop();
    reset();
  }, [reset]);

  const startBrowserMeter = useCallback((stream: MediaStream): boolean => {
    stopBrowserMeter();
    if (typeof window === "undefined") return false;
    const AudioContextConstructor = window.AudioContext || (window as BrowserWindowWithAudioContext).webkitAudioContext;
    if (!AudioContextConstructor) return false;

    let rafId: number | null = null;
    try {
      const audioContext = new AudioContextConstructor();
      const source = audioContext.createMediaStreamSource(stream);
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 512;
      analyser.smoothingTimeConstant = 0.45;
      source.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);

      const tick = () => {
        analyser.getFloatTimeDomainData(samples);
        updateFromSamples(samples);
        rafId = window.requestAnimationFrame(tick);
      };
      tick();

      browserMeterRef.current = {
        stop: () => {
          if (rafId !== null) {
            window.cancelAnimationFrame(rafId);
            rafId = null;
          }
          try {
            source.disconnect();
          } catch {
            // ignore browser audio meter cleanup failure
          }
          closeAudioContext(audioContext);
        },
      };
      return true;
    } catch {
      if (rafId !== null) window.cancelAnimationFrame(rafId);
      reset();
      return false;
    }
  }, [reset, stopBrowserMeter, updateFromSamples]);

  return useMemo(() => ({
    levels,
    reset,
    startBrowserMeter,
    stopBrowserMeter,
    updateFromSamples,
  }), [levels, reset, startBrowserMeter, stopBrowserMeter, updateFromSamples]);
}
