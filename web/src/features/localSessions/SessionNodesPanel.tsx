import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SessionNodeDirectoryNotice } from "./SessionNodeDirectoryNotice";
import { SessionNodeManagement } from "./SessionNodeManagement";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { SessionNodeRelay } from "./SessionNodeRelay";
import { useSessionNodes } from "./useSessionNodes";
import { useSessionNodeTicket } from "./useSessionNodeTicket";

type PanelMode = "picker" | "settings";

type PanelProps = {
  mode: PanelMode;
  /** Picker only: leave the pairing subview and return to the session list. */
  onBackToSessions?: () => void;
};

/** One authorized directory and one in-memory ticket per mounted panel. */
function DeviceViews({
  actions,
  mode,
}: {
  mode: PanelMode;
  actions: ReturnType<typeof useSessionNodes>;
}) {
  const { t } = useTranslation("layout");
  const [pairing, setPairing] = useState(mode === "picker");
  const [confirmStop, setConfirmStop] = useState(false);
  const endpointLive = actions.directory.status === "ready" && !!actions.directory.endpoint;
  useEffect(() => {
    if (!endpointLive) setConfirmStop(false);
  }, [endpointLive]);
  const [query, setQuery] = useState("");
  const [ticketError, setTicketError] = useState("");
  const invitation = useSessionNodeTicket(setTicketError, actions.forbid);
  const busy =
    actions.revoking ||
    actions.relayBusy ||
    actions.stopping ||
    invitation.creating ||
    invitation.copying;
  if (actions.directory.status !== "ready")
    return <SessionNodeDirectoryNotice status={actions.directory.status} />;
  const { directory } = actions;
  const handleToggleRelay = async (enabled: boolean) => {
    const ok = await actions.toggleRelay(enabled);
    if (ok) invitation.clear();
  };
  const handleStopGateway = async () => {
    const ok = await actions.stopGateway();
    if (ok) {
      setConfirmStop(false);
      invitation.clear();
    }
  };
  const preparation = (
    <SessionNodePreparation
      invitation={invitation}
      supportedRuntimes={directory.supportedRuntimes}
      ticketError={ticketError}
      busy={busy}
      directoryHintKey={
        mode === "picker"
          ? "sessionNodes.ticketDirectoryHintPicker"
          : "sessionNodes.ticketDirectoryHint"
      }
    />
  );
  const relay = (
    <SessionNodeRelay
      relay={directory.relay}
      relayBusy={actions.relayBusy}
      relayError={actions.relayError}
      busy={busy}
      onToggleRelay={(enabled) => void handleToggleRelay(enabled)}
    />
  );
  if (mode === "picker")
    return (
      <fieldset disabled={busy} className="min-w-0 space-y-3 border-0 p-0">
        {preparation}
        {relay}
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t("sessionNodes.manageInSettings")}
        </p>
      </fieldset>
    );
  return (
    <fieldset disabled={busy} className="min-w-0 space-y-3 border-0 p-0">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-sm font-medium">{t("sessionNodes.devices")}</h4>
        <Button
          type="button"
          size="sm"
          variant={pairing ? "outline" : "default"}
          aria-expanded={pairing}
          className="min-h-9 max-sm:min-h-11 text-xs"
          onClick={() => setPairing((open) => !open)}
        >
          {t("sessionNodes.pairComputer")}
        </Button>
      </div>
      {pairing && preparation}
      <SessionNodeManagement
        devices={directory.devices}
        actions={actions}
        query={query}
        onSearch={setQuery}
      />
      {relay}
      {directory.endpoint && (
        <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3">
          {confirmStop ? (
            <div className="space-y-2 rounded-lg border border-[var(--color-border-primary)] p-2.5">
              <p className="text-sm">{t("sessionNodes.stopGatewayConfirm")}</p>
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="max-sm:min-h-11"
                  disabled={busy}
                  onClick={() => setConfirmStop(false)}
                >
                  {t("common:cancel")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="destructive"
                  className="max-sm:min-h-11"
                  disabled={busy}
                  onClick={() => void handleStopGateway()}
                >
                  {t(
                    actions.stopping ? "sessionNodes.stoppingGateway" : "sessionNodes.stopGateway",
                  )}
                </Button>
              </div>
            </div>
          ) : (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("sessionNodes.gatewayRunning")}
              </p>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="min-h-9 max-sm:min-h-11 text-xs"
                disabled={busy}
                onClick={() => setConfirmStop(true)}
              >
                {t("sessionNodes.stopGateway")}
              </Button>
            </div>
          )}
          {actions.stopError && (
            <p role="alert" className="text-sm text-[var(--color-accent-danger)]">
              {t("sessionNodes.stopGatewayFailed")}
            </p>
          )}
        </div>
      )}
    </fieldset>
  );
}

export function SessionNodesPanel({
  mode,
  onBackToSessions,
  active = true,
}: PanelProps & { active?: boolean }) {
  const { t } = useTranslation("layout");
  const actions = useSessionNodes(active);
  const { directory } = actions;
  // An outage retains the ticket for a retry. Losing access unmounts the views and discards it.
  const body =
    directory.status === "ready" ||
    directory.status === "failed" ||
    directory.status === "rechecking" ? (
      <DeviceViews actions={actions} mode={mode} />
    ) : (
      <SessionNodeDirectoryNotice status={directory.status} />
    );
  if (mode !== "picker") return body;
  // The way back stays available in every directory state, so the subview never traps the picker.
  return (
    <div className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--glass-border-subtle)] pb-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="min-h-10 max-sm:min-h-11 -ml-2 text-sm"
          onClick={onBackToSessions}
        >
          {t("sessionNodes.backToSessionList")}
        </Button>
      </div>
      {body}
    </div>
  );
}
