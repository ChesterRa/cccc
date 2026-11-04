import { useEffect } from "react";
import { useComposerStore } from "../../stores/useComposerStore";

/**
 * Sends waiting to be restored live only in this page. While any Group has one, leaving or
 * reloading the page asks first. It cannot protect against a crash or a killed browser.
 */
export function FailedSendsUnloadGuard() {
  const pending = useComposerStore((state) =>
    Object.values(state.failedSendsByGroup).some((items) => items.length > 0),
  );
  useEffect(() => {
    if (!pending) return;
    const protect = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", protect);
    return () => window.removeEventListener("beforeunload", protect);
  }, [pending]);
  return null;
}
