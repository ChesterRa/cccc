import { useEffect, useState } from "react";

const QUERY = "(pointer: coarse) and (max-width: 1023px) and (max-height: 480px)";

/** Treat a short touch landscape as compact without changing desktop layout. */
export function useShortTouchViewport(): boolean {
  const [matches, setMatches] = useState(
    () => typeof window !== "undefined" && !!window.matchMedia?.(QUERY).matches,
  );
  useEffect(() => {
    if (!window.matchMedia) return;
    const media = window.matchMedia(QUERY);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return matches;
}
