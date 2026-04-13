import { useCallback, useEffect, useMemo, useRef } from "react";

type LatestRequestOptions<T> = {
  run: (signal: AbortSignal) => Promise<T>;
  onSuccess?: (value: T) => void | Promise<void>;
  onError?: (error: unknown) => void | Promise<void>;
  onSettled?: () => void | Promise<void>;
};

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

export function useLatestRequest() {
  const requestSeqRef = useRef(0);
  const abortRef = useRef<AbortController | null>(null);

  const cancelLatest = useCallback(() => {
    requestSeqRef.current += 1;
    abortRef.current?.abort();
    abortRef.current = null;
  }, []);

  const runLatest = useCallback(async <T,>(options: LatestRequestOptions<T>): Promise<T | null> => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    const requestSeq = requestSeqRef.current + 1;
    requestSeqRef.current = requestSeq;

    try {
      const value = await options.run(controller.signal);
      if (requestSeqRef.current !== requestSeq || controller.signal.aborted) {
        return null;
      }
      await options.onSuccess?.(value);
      return value;
    } catch (error) {
      if (requestSeqRef.current !== requestSeq || controller.signal.aborted || isAbortError(error)) {
        return null;
      }
      await options.onError?.(error);
      throw error;
    } finally {
      if (requestSeqRef.current === requestSeq) {
        await options.onSettled?.();
      }
      if (abortRef.current === controller) {
        abortRef.current = null;
      }
    }
  }, []);

  useEffect(() => cancelLatest, [cancelLatest]);

  return useMemo(() => ({ runLatest, cancelLatest }), [cancelLatest, runLatest]);
}
