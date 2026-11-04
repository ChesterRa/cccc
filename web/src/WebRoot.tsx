import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { FailedSendsUnloadGuard } from "./components/app/FailedSendsUnloadGuard";
import { CapabilityCenterStandaloneApp } from "./components/capabilities/CapabilityCenterStandaloneApp";
import { isCapabilityCenterPath } from "./components/capabilities/capabilityCenterRoute";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";

/**
 * The document root. Sends waiting to be restored stay in memory for the whole page, while a
 * sign-in prompt or an expired Connect frame unmounts the app, so the leave warning sits here,
 * beside every branch rather than inside one.
 */
export function WebRoot({ pathname }: { pathname: string }) {
  return (
    <>
      <FailedSendsUnloadGuard />
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
