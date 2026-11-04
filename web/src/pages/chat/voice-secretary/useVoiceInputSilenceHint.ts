import { useCallback, useEffect, useRef, useState } from "react";

const NO_SIGNAL_MS = 6_000;

/** A hint only: no frames or exact zero samples do not establish a device fault. */
export function useVoiceInputSilenceHint() {
  const [silent, setSilent] = useState(false);
  const runId = useRef(0);
  const lastSignal = useRef(0);
  const showing = useRef(false);
  const timer = useRef<number | null>(null);

  const stop = useCallback((id: number) => {
    if (runId.current !== id) return;
    runId.current = 0;
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    if (showing.current) {
      showing.current = false;
      setSilent(false);
    }
  }, []);

  const start = useCallback((id: number) => {
    if (timer.current !== null) window.clearInterval(timer.current);
    runId.current = id;
    lastSignal.current = performance.now();
    if (showing.current) {
      showing.current = false;
      setSilent(false);
    }
    timer.current = window.setInterval(() => {
      if (!showing.current && performance.now() - lastSignal.current >= NO_SIGNAL_MS) {
        showing.current = true;
        setSilent(true);
      }
    }, 1_000);
  }, []);

  const observe = useCallback((id: number, samples: Float32Array) => {
    if (runId.current !== id) return;
    // Do not use the visual meter: it intentionally maps quiet non-zero input to zero.
    if (!samples.some((sample) => sample !== 0)) return;
    lastSignal.current = performance.now();
    if (showing.current) {
      showing.current = false;
      setSilent(false);
    }
  }, []);

  useEffect(
    () => () => {
      runId.current = 0;
      if (timer.current !== null) window.clearInterval(timer.current);
      timer.current = null;
    },
    [],
  );
  return { silent, start, observe, stop };
}
