import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
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
          <App />
        </AuthGate>
      )}
    </>
  );
}
