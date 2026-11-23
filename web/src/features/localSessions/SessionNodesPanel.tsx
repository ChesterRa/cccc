import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ActorConfigTabs } from "../../components/modals/ActorConfigTabs";
import { Button } from "../../components/ui/button";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";
import { SessionNodeDirectoryNotice } from "./SessionNodeDirectoryNotice";
import { SessionNodeManagement } from "./SessionNodeManagement";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { useSessionNodes } from "./useSessionNodes";
import { useSessionNodeTicket } from "./useSessionNodeTicket";

type PanelProps = { onChooseSessions: (device: SessionNodeDevice) => void };

/** Both tabs share one authorized directory and one in-memory ticket. */
function DeviceViews({
  actions,
  onChooseSessions,
}: PanelProps & { actions: ReturnType<typeof useSessionNodes> }) {
  const { t } = useTranslation("layout");
  const [view, setView] = useState("pair");
  const [confirmStop, setConfirmStop] = useState(false);
  const endpointLive = actions.directory.status === "ready" && !!actions.directory.endpoint;
  useEffect(() => {
    if (!endpointLive) setConfirmStop(false);
  }, [endpointLive]);
  const switcher = useRef<HTMLFieldSetElement>(null);
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
  return (
    <fieldset
      ref={switcher}
      disabled={busy}
      className="min-w-0 space-y-3 border-0 p-0 [&_[role=tab]]:min-h-10 [&_[role=tab]]:flex-1 max-sm:[&_[role=tab]]:min-h-11"
    >
      <ActorConfigTabs
        ariaLabel={t("localSessions.addDevice")}
        activeId={view}
        onChange={(next) => {
          if (!busy) {
            actions.setRevokeId("");
            setConfirmStop(false);
            setView(next);
          }
        }}
        tabs={[
          {
            id: "pair",
            label: t("localSessions.addDevice"),
            panel: (
              <div className="min-w-0 space-y-3">
                <SessionNodePreparation
                  invitation={invitation}
                  relay={directory.relay}
                  supportedRuntimes={directory.supportedRuntimes}
                  relayBusy={actions.relayBusy}
                  relayError={actions.relayError}
                  onToggleRelay={(enabled) => void handleToggleRelay(enabled)}
                  ticketError={ticketError}
                  busy={busy}
                  onChooseDeviceSessions={() => {
                    setQuery("");
                    actions.setRevokeId("");
                    setView("devices");
                    requestAnimationFrame(() =>
                      switcher.current
                        ?.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')
                        ?.focus({ preventScroll: true }),
                    );
                  }}
                />
              </div>
            ),
          },
          {
            id: "devices",
            label: t("sessionNodes.devices"),
            panel:
              view === "devices" ? (
                <SessionNodeManagement
                  devices={directory.devices}
                  actions={actions}
                  query={query}
                  onSearch={setQuery}
                  onChooseSessions={onChooseSessions}
                />
              ) : null,
          },
        ]}
      />
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
  onChooseSessions,
  active = true,
}: PanelProps & { active?: boolean }) {
  const actions = useSessionNodes(active);
  const { directory } = actions;
  // An outage retains the ticket for a retry. Losing access unmounts both tabs and discards it.
  if (
    directory.status === "ready" ||
    directory.status === "failed" ||
    directory.status === "rechecking"
  )
    return <DeviceViews actions={actions} onChooseSessions={onChooseSessions} />;
  return <SessionNodeDirectoryNotice status={directory.status} />;
}
