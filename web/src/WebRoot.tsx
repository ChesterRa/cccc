import { useCallback, useEffect, useState } from "react";
import App from "./App";
import { AuthGate } from "./components/AuthGate";
import { PendingPageWorkUnloadGuard } from "./components/app/PendingPageWorkUnloadGuard";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";
import { SessionOverviewRequest } from "./features/localSessions/useSessionDialogNavigation";
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
  const [openOverview, setOpenOverview] = useState(false);
  const consumeOverview = useCallback(() => setOpenOverview(false), []);
  useEffect(() => {
    const sync = () => {
      setOpenOverview(false);
      setRequestId(linkedSessionNodeJoin());
    };
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
          <SessionNodeJoinDialog
            key={requestId}
            requestId={requestId}
            onClose={closeReview}
            onChooseSessions={() => {
              setOpenOverview(true);
              closeReview();
            }}
          />
        </AuthGate>
      ) : (
        <AuthGate>
          <SessionOverviewRequest.Provider value={openOverview ? consumeOverview : null}>
            <App />
          </SessionOverviewRequest.Provider>
        </AuthGate>
      )}
    </>
  );
}
