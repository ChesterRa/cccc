import { useEffect, useState } from "react";

import type { TextScale } from "../types";
import {
  applyTextScale,
  getStoredTextScale,
  normalizeTextScale,
  TEXT_SCALE_STORAGE_KEY,
} from "../utils/textScale";

export function useTextScale() {
  const [textScale, setTextScaleState] = useState<TextScale>(getStoredTextScale);

  useEffect(() => {
    window.localStorage.setItem(TEXT_SCALE_STORAGE_KEY, String(applyTextScale(textScale)));
  }, [textScale]);

  const setTextScale = (value: TextScale) => {
    setTextScaleState(normalizeTextScale(value));
  };

  return { textScale, setTextScale };
}
