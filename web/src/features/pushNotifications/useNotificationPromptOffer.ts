import { useCallback, useEffect, useState } from "react";
import { PUSH_STATUS_EVENT, pushSupported, readPushSetupStatus } from "./subscription";

const DISMISSED_KEY = "cccc.notificationPrompt.dismissed";
const OFFERED = new Set(["disabled", "unbound", "keyMismatch"]);

function readDismissed(): boolean {
  try {
    return window.localStorage.getItem(DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * Offers system notifications when they would help: requests are waiting
 * here, this browser can receive them, and they are not on for this account.
 * The caller renders the offer, so its layout can make room for the row.
 */
export function useNotificationPromptOffer(waiting: boolean) {
  const [dismissed, setDismissed] = useState(readDismissed);
  const [offer, setOffer] = useState(false);
  const active = waiting && !dismissed && pushSupported();

  useEffect(() => {
    if (!active) {
      setOffer(false);
      return;
    }
    let closed = false;
    let latest = 0;
    // A reading that cannot be confirmed offers nothing.
    const check = () => {
      const reading = ++latest;
      void readPushSetupStatus().then(
        (status) => {
          if (!closed && reading === latest) setOffer(OFFERED.has(status));
        },
        () => {
          if (!closed && reading === latest) setOffer(false);
        },
      );
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") check();
    };
    check();
    window.addEventListener("focus", check);
    window.addEventListener(PUSH_STATUS_EVENT, check);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      closed = true;
      window.removeEventListener("focus", check);
      window.removeEventListener(PUSH_STATUS_EVENT, check);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [active]);

  // Not now in another tab of this browser hides the offer here too.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === DISMISSED_KEY || event.key === null) setDismissed(readDismissed());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const dismiss = useCallback(() => {
    try {
      window.localStorage.setItem(DISMISSED_KEY, "1");
    } catch {
      // Without storage the offer stays hidden until the page reloads.
    }
    setDismissed(true);
  }, []);
  return { visible: active && offer, dismiss };
}
