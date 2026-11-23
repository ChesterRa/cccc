import { useEffect, useRef } from "react";
import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";

export function WebRoot({
  pathname,
  onWorkbenchEntry,
}: {
  pathname: string;
  onWorkbenchEntry?: () => void;
}) {
  const initialized = useRef(false);
  useEffect(() => {
    if (!initialized.current) {
      initialized.current = true;
      onWorkbenchEntry?.();
    }
  }, [onWorkbenchEntry]);
  return (
    <>
      <PendingPageWorkUnloadGuard />
      {isConnectFramePath(pathname) ? (
        <ConnectEmbeddedApp />
      ) : (
        <AuthGate>
          <App />
        </AuthGate>
      )}
    </>
  );
}
