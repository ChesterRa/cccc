// useIMEComposition — Fix IME composition interruption in controlled inputs.
//
// Problem: Zustand uses useSyncExternalStore which triggers synchronous
// re-renders, bypassing React 18's auto-batching. During IME composition,
// this causes the controlled value to overwrite the DOM mid-composition,
// breaking CJK input.
//
// Solution: Derive the display value from composition state — use the
// buffered local value during composition, and the external value otherwise.
// This avoids useEffect-based sync entirely.

import { useCallback, useRef, useState } from "react";

type InputElement = HTMLInputElement | HTMLTextAreaElement;

interface UseIMECompositionOptions {
  /** The controlled value from external state (e.g. Zustand). */
  value: string;
  /** The external setter — called only outside composition or on compositionend. */
  onChange: (value: string) => void;
}

interface UseIMECompositionReturn {
  /** Bind this to the element's value prop. Immune to external re-renders during composition. */
  value: string;
  /** Bind to onChange. */
  onChange: (e: React.ChangeEvent<InputElement>) => void;
  /** Bind to onCompositionStart. */
  onCompositionStart: () => void;
  /** Bind to onCompositionEnd. */
  onCompositionEnd: (e: React.CompositionEvent<InputElement>) => void;
}

export function useIMEComposition({
  value,
  onChange,
}: UseIMECompositionOptions): UseIMECompositionReturn {
  const [localValue, setLocalValue] = useState(value);
  const [isComposing, setIsComposing] = useState(false);
  const composingRef = useRef(false);

  // Derive display value: during composition use buffered local value,
  // otherwise use external value directly (no sync needed).
  const displayValue = isComposing ? localValue : value;

  const handleChange = useCallback(
    (e: React.ChangeEvent<InputElement>) => {
      const nextValue = e.target.value;
      setLocalValue(nextValue);

      if (!composingRef.current) {
        // Not composing: propagate to external state immediately.
        onChange(nextValue);
      }
      // Composing: only update local state. External state stays untouched
      // until compositionEnd flushes the final value.
    },
    [onChange],
  );

  const handleCompositionStart = useCallback(() => {
    composingRef.current = true;
    setIsComposing(true);
  }, []);

  const handleCompositionEnd = useCallback(
    (e: React.CompositionEvent<InputElement>) => {
      composingRef.current = false;
      setIsComposing(false);
      // Flush the final composed value to external state.
      const finalValue = e.currentTarget.value;
      setLocalValue(finalValue);
      onChange(finalValue);
    },
    [onChange],
  );

  return {
    value: displayValue,
    onChange: handleChange,
    onCompositionStart: handleCompositionStart,
    onCompositionEnd: handleCompositionEnd,
  };
}
