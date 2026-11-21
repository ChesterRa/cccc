import { useEffect, useState } from "react";
import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";
import { SessionNodeJoinDialog } from "./features/localSessions/SessionNodeJoinDialog";
import {
  clearSessionNodeJoinLink,
  linkedSessionNodeJoin,
} from "./features/localSessions/sessionNodeJoin";

/**
 * The document root outlives workbench branches, so the leave warning also covers work that
 * settles after a sign-in prompt or an expired Connect frame unmounts the workbench.
 */
export function WebRoot({ pathname }: { pathname: string }) {
  const [requestId, setRequestId] = useState(linkedSessionNodeJoin);
  useEffect(() => {
    const sync = () => setRequestId(linkedSessionNodeJoin());
    window.addEventListener("popstate", sync);
    return () => window.removeEventListener("popstate", sync);
  }, []);
  const closeReview = () => {
    clearSessionNodeJoinLink();
    setRequestId(null);
  };
  return (
    <>
      <PendingPageWorkUnloadGuard />
      {isConnectFramePath(pathname) ? (
        <ConnectEmbeddedApp />
      ) : requestId !== null ? (
        <AuthGate key="device-review" requireAdmin>
          <SessionNodeJoinDialog key={requestId} requestId={requestId} onClose={closeReview} />
        </AuthGate>
      ) : (
        <AuthGate>
          <App />
        </AuthGate>
      )}
    </>
  );
}
