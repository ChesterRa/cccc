import { useEffect } from "react";
import { fetchWebMods, isModSafeMode, reconcileModStyles } from "./webMods";

/** While any mod is installed, edits show within this time. */
export const WEB_MODS_POLL_MS = 1000;
/** With no mod installed, the page only checks now and then. */
export const WEB_MODS_IDLE_POLL_MS = 30_000;

/**
 * Applies the instance's enabled style mods to this page and keeps them current.
 * A visible page re-reads the listing (quickly while any mod is installed, rarely
 * otherwise, and at once when it becomes visible again) and re-applies when the
 * revision moves. In safe mode (`?safe=1`) it loads nothing.
 */
export function useWebMods(): void {
  useEffect(() => {
    if (isModSafeMode(window.location.search)) return;
    let revision = "";
    let installed = false;
    let stopped = false;
    let inFlight = false;
    let timer: number | undefined;

    const schedule = () => {
      if (stopped) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(
        () => void refresh(),
        installed ? WEB_MODS_POLL_MS : WEB_MODS_IDLE_POLL_MS,
      );
    };
    const refresh = async () => {
      if (stopped || inFlight) return;
      if (document.hidden) return schedule();
      inFlight = true;
      try {
        const listing = await fetchWebMods();
        if (stopped || !listing) return;
        installed = listing.mods.length > 0;
        if (listing.revision === revision) return;
        revision = listing.revision;
        reconcileModStyles(document, listing);
      } finally {
        inFlight = false;
        schedule();
      }
    };
    const onVisible = () => {
      if (!document.hidden) void refresh();
    };
    void refresh();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);
}
