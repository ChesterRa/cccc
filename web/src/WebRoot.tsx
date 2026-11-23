import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";

const hasOldDeviceLink = () =>
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).has("session_node_join");

/** Older connectors may still open this URL. Do not mount an authenticated surface on arrival. */
function OldDeviceLink() {
  const { t } = useTranslation("layout");
  return (
    <main className="grid min-h-dvh place-items-center bg-[var(--color-bg-primary)] p-5 text-[var(--color-text-primary)]">
      <section className="max-w-md space-y-3">
        <h1 className="text-xl font-semibold">{t("sessionNodes.oldLinkTitle")}</h1>
        <p className="leading-relaxed text-[var(--color-text-secondary)]">
          {t("sessionNodes.oldLinkHelp")}
        </p>
      </section>
    </main>
  );
}

export function WebRoot({
  pathname,
  onWorkbenchEntry,
}: {
  pathname: string;
  onWorkbenchEntry?: () => void;
}) {
  const [oldDeviceLink, setOldDeviceLink] = useState(hasOldDeviceLink);
  const initialized = useRef(false);
  useEffect(() => {
    if (!oldDeviceLink && !initialized.current) {
      initialized.current = true;
      onWorkbenchEntry?.();
    }
  }, [oldDeviceLink, onWorkbenchEntry]);
  useEffect(() => {
    const sync = () => setOldDeviceLink(hasOldDeviceLink());
    window.addEventListener("popstate", sync);
    return () => window.removeEventListener("popstate", sync);
  }, []);
  return (
    <>
      <PendingPageWorkUnloadGuard />
      {oldDeviceLink ? (
        <OldDeviceLink />
      ) : isConnectFramePath(pathname) ? (
        <ConnectEmbeddedApp />
      ) : (
        <AuthGate>
          <App />
        </AuthGate>
      )}
    </>
  );
}
