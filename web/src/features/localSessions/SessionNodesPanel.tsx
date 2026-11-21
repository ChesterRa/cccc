import { useState } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";
import { SessionNodeDevices } from "./SessionNodeDevices";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { useSessionNodes } from "./useSessionNodes";
import { useSessionNodeTicket } from "./useSessionNodeTicket";

type DirectoryActions = ReturnType<typeof useSessionNodes>;

/** A loss of device-management access unmounts this view and discards its private ticket. */
function DevicesWithAccess({
  devices,
  actions,
  onChooseSessions,
}: {
  devices: SessionNodeDevice[] | null;
  actions: DirectoryActions;
  onChooseSessions: () => void;
}) {
  const { t } = useTranslation("layout");
  const [managed, setManaged] = useState(false);
  const [ticketError, setTicketError] = useState("");
  const invitation = useSessionNodeTicket(setTicketError, actions.forbid);
  const busy = actions.revoking || invitation.creating || invitation.copying;
  // A temporary network failure disables actions but keeps any unexpired ticket for a retry.
  if (!devices) return <p role="alert">{t("sessionNodes.directoryFailed")}</p>;
  return (
    <div className="min-w-0 space-y-3">
      <SessionNodePreparation invitation={invitation} busy={busy} />
      {ticketError && (
        <p role="alert" className="text-sm text-[var(--color-danger)]">
          {ticketError}
        </p>
      )}
      {devices.length > 0 && (
        <div className="border-t border-[var(--glass-border-subtle)]">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-expanded={managed}
            className="-ml-2 mt-1 min-h-10 max-sm:min-h-11"
            onClick={() => setManaged(!managed)}
          >
            <ChevronDown size={14} aria-hidden="true" className={managed ? "rotate-180" : ""} />
            {t("sessionNodes.ticketManage")}
          </Button>
          {managed && (
            <SessionNodeDevices
              devices={devices}
              busy={busy}
              revokeId={actions.revokeId}
              setRevokeId={actions.setRevokeId}
              revoke={actions.revoke}
              onChooseSessions={onChooseSessions}
            />
          )}
        </div>
      )}
      {actions.revokeError && (
        <p role="alert" className="text-sm text-[var(--color-danger)]">
          {t("sessionNodes.revokeFailed")}
        </p>
      )}
    </div>
  );
}

export function SessionNodesPanel({ onChooseSessions }: { onChooseSessions: () => void }) {
  const { t } = useTranslation("layout");
  const actions = useSessionNodes();
  const { directory } = actions;
  if (directory.status === "ready" || directory.status === "failed")
    return (
      <DevicesWithAccess
        devices={directory.status === "ready" ? directory.devices : null}
        actions={actions}
        onChooseSessions={onChooseSessions}
      />
    );
  if (directory.status === "unsupported")
    return (
      <div role="status" className="space-y-1">
        <h3 className="text-base font-semibold">{t("sessionNodes.backendUpgradeTitle")}</h3>
        <p className="text-sm text-[var(--color-text-secondary)]">
          {t("sessionNodes.upgradeRequired")}
        </p>
      </div>
    );
  if (directory.status === "loading")
    return <p role="status">{t("sessionNodes.checkingBackend")}</p>;
  return <p role="alert">{t("sessionNodes.adminRequired")}</p>;
}
