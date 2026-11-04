import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
import { CapabilityCenterStandaloneApp } from "./components/capabilities/CapabilityCenterStandaloneApp";
import { isCapabilityCenterPath } from "./components/capabilities/capabilityCenterRoute";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";

/**
 * The document root outlives workbench branches, so the leave warning also covers work that
 * settles after a sign-in prompt or an expired Connect frame unmounts the workbench.
 */
export function WebRoot({ pathname }: { pathname: string }) {
  return (
    <>
      <PendingPageWorkUnloadGuard />
      {isConnectFramePath(pathname) ? (
        <ConnectEmbeddedApp />
      ) : (
        <AuthGate>
          {isCapabilityCenterPath(pathname) ? <CapabilityCenterStandaloneApp /> : <App />}
        </AuthGate>
      )}
    </>
  );
}
