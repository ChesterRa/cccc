import { useMediaQuery } from "./useMediaQuery";

const QUERY = "(pointer: coarse) and (max-width: 1023px) and (max-height: 480px)";

/** Treat a short touch landscape as compact without changing desktop layout. */
export function useShortTouchViewport(): boolean {
  return useMediaQuery(QUERY);
}
