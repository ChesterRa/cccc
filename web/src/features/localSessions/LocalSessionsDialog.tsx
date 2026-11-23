import { useState } from "react";
import { ArrowLeft, Laptop } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { Button } from "../../components/ui/button";
import type { LocalSessionsHelpRequest } from "./LocalSessionsHelp";
import { LocalSessionsGuide } from "./LocalSessionsGuide";
import { LocalSessionsSelection } from "./LocalSessionsSelection";
import { LocalSessionsWelcome } from "./LocalSessionsWelcome";
import { LocalSessionsConfirmation } from "./LocalSessionsConfirmation";
import { useLocalSessionsConnection } from "./useLocalSessionsConnection";
import { SessionNodesPanel } from "./SessionNodesPanel";
import { SessionNodeHubLabel } from "./SessionNodeProgress";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";
import { useSessionDialogNavigation } from "./useSessionDialogNavigation";

export function LocalSessionsDialog({
  isDark,
  onClose,
  onSelectGroup,
  initialTargetGroupId,
  hubName,
}: {
  isDark: boolean;
  onClose: () => void;
  onSelectGroup: (id: string) => void;
  initialTargetGroupId?: string;
  hubName?: string;
}) {
  const { t } = useTranslation("layout");
  const [deviceBrowser, setDeviceBrowser] = useState<SessionNodeDevice | "all" | null>(null);
  const deviceScope = typeof deviceBrowser === "object" ? deviceBrowser : null;
  const { modalRef } = useModalA11y(true, onClose);
  const navigation = useSessionDialogNavigation();
  const { view, navigate } = navigation;
  const deviceSessions = view === "sessions" && deviceBrowser !== null;
  const compactDialog = view === "device" || deviceSessions || view === "group";
  function openDeviceSessions(device: SessionNodeDevice) {
    setDeviceBrowser(device);
    navigate("sessions");
  }
  function backToDevices() {
    setDeviceBrowser(null);
    navigate("device");
  }
  const connection = useLocalSessionsConnection({
    onClose,
    onSelectGroup,
    initialTargetGroupId,
    reviewing: view === "group",
    viewingSessions: view === "sessions",
    deviceId: deviceScope?.node_id,
    browsingDeviceSessions: view === "sessions" && !!deviceScope,
  });
  const unjoinedDeviceSources = deviceScope
    ? connection.displayedSources.filter((source) => !connection.members.has(source.source_id))
    : [];
  const deviceHint = !connection.displayedSources.length
    ? "sessionNodes.noDeviceSessions"
    : !connection.membersReady || !unjoinedDeviceSources.length
      ? null
      : unjoinedDeviceSources.some((source) => source.connected)
        ? "sessionNodes.deviceSessionsHint"
        : "sessionNodes.noDeviceSessions";
  const hasSources =
    connection.displayedSources.length > 0 || !!connection.directory.removed_sources?.length;
  const empty = !connection.loading && !connection.error && !hasSources;
  const panelWidth =
    view === "sessions" && !hasSources
      ? "sm:max-w-2xl"
      : view === "help"
        ? "sm:max-w-4xl"
        : "sm:max-w-3xl";
  const [helpRequest, setHelpRequest] = useState<LocalSessionsHelpRequest | null>(null);
  const showHelp = (section: LocalSessionsHelpRequest["section"]) => {
    setHelpRequest((previous) => ({ section, sequence: (previous?.sequence || 0) + 1 }));
    navigate("help");
  };
  const canContinue =
    !connection.busy &&
    !connection.loading &&
    !connection.error &&
    connection.selected.length > 0 &&
    connection.selected.length <= connection.selectionLimit &&
    connection.unavailableCount === 0;
  const reviewAction = connection.targetPending
    ? "localSessions.chooseTarget"
    : connection.creatingGroup
      ? connection.busy
        ? "localSessions.creating"
        : "localSessions.create"
      : connection.busy
        ? "localSessions.joining"
        : "localSessions.join";
  const title = t(
    view === "sessions"
      ? "localSessions.overviewTitle"
      : view === "group"
        ? connection.targetPending
          ? "localSessions.chooseTarget"
          : connection.creatingGroup
            ? "createNewGroup"
            : "localSessions.chooseGroup"
        : view === "device"
          ? "localSessions.addDevice"
          : "localSessions.connectionHelp",
  );
  const heading = (
    <h2
      ref={navigation.titleRef}
      tabIndex={-1}
      className={
        view === "device"
          ? "text-2xl font-semibold outline-none"
          : "text-lg font-semibold outline-none"
      }
    >
      {title}
    </h2>
  );
  return (
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      titleId="local-sessions-title"
      title={
        view === "device" ? (
          <div className="min-w-0">
            <SessionNodeHubLabel instanceName={hubName} />
            {heading}
          </div>
        ) : (
          heading
        )
      }
      closeAriaLabel={t("common:close")}
      modalRef={modalRef}
      panelClassName={
        compactDialog
          ? "self-center h-auto max-h-[calc(100dvh_-_1rem)] w-[calc(100vw_-_1rem)] max-w-[554px] rounded-xl sm:max-h-[88dvh]"
          : view === "sessions" && !hasSources
            ? "self-center h-auto max-h-[88dvh] w-full max-w-[calc(100vw_-_2rem)] sm:max-w-2xl"
            : `h-[100dvh] w-full sm:h-auto sm:max-h-[88dvh] ${panelWidth}`
      }
      surface="solid"
      headerActions={
        (deviceSessions || view === "device" || view === "help") && (
          <Button
            variant="ghost"
            size="sm"
            className="text-sm max-sm:min-h-11"
            aria-label={t(deviceSessions ? "sessionNodes.devices" : "localSessions.backToSessions")}
            onClick={deviceSessions ? backToDevices : () => navigate("sessions")}
          >
            <LcdIcon icon={ArrowLeft} glyph="chevron-left" size={16} aria-hidden="true" />
            <span className="hidden sm:inline">
              {t(deviceSessions ? "sessionNodes.devices" : "localSessions.backToSessions")}
            </span>
          </Button>
        )
      }
      footerActions={
        ((view === "sessions" && hasSources) || view === "group") && (
          <div
            className={
              view === "group"
                ? "flex flex-wrap items-center justify-between gap-2"
                : "flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between"
            }
          >
            {view === "group" ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="min-h-11 text-sm"
                disabled={connection.busy}
                onClick={() => navigate("sessions")}
              >
                <LcdIcon icon={ArrowLeft} glyph="chevron-left" size={16} aria-hidden="true" />
                {t("localSessions.backToSessions")}
              </Button>
            ) : (
              <p role="status" className="text-sm text-[var(--color-text-secondary)]">
                {t("localSessions.selectedCount", {
                  count: connection.selected.length,
                  limit: connection.selectionLimit,
                })}
              </p>
            )}
            <Button
              disabled={
                view === "sessions"
                  ? !canContinue
                  : connection.targetPending || !connection.canSubmit
              }
              aria-busy={connection.busy}
              onClick={() => (view === "sessions" ? navigate("group") : void connection.start())}
            >
              {t(view === "sessions" ? "localSessions.continue" : reviewAction)}
            </Button>
          </div>
        )
      }
    >
      <div
        ref={navigation.contentRef}
        className="min-h-0 flex-1 overflow-y-auto text-sm"
        aria-busy={connection.loading || connection.busy}
        data-lcd-scope
      >
        <div
          className={
            view === "device" ? "min-w-0 px-4 pt-3 pb-4 sm:px-6 sm:pb-5" : "space-y-5 p-4 sm:p-6"
          }
        >
          {view === "sessions" && deviceScope && (
            <div className="min-w-0 space-y-1.5 border-b border-[var(--glass-border-subtle)] pb-2">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <LcdIcon
                  icon={Laptop}
                  glyph="server"
                  size={16}
                  aria-hidden="true"
                  className="shrink-0"
                />
                <span dir="auto" className="min-w-0 flex-1 break-words font-medium">
                  {deviceScope.name}
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setDeviceBrowser("all")}
                >
                  {t("sessionNodes.allSessions")}
                </Button>
              </div>
              {!connection.loading && !connection.error && deviceHint && (
                <p
                  role="status"
                  className="text-sm leading-relaxed text-[var(--color-text-secondary)]"
                >
                  {t(deviceHint, { action: t("localSessions.continue"), device: deviceScope.name })}
                </p>
              )}
            </div>
          )}
          <LocalSessionsGuide
            view={view}
            showEntryActions={deviceBrowser === null && (hasSources || !!connection.error)}
            request={helpRequest}
            onAddDevice={() => navigate("device")}
            onShowHelp={() => showHelp("setup")}
            onChooseSessions={() => navigate("sessions")}
          />
          {(view === "device" || deviceBrowser !== null) && (
            <div hidden={view !== "device"}>
              <SessionNodesPanel active={view === "device"} onChooseSessions={openDeviceSessions} />
            </div>
          )}
          <div hidden={view !== "sessions"}>
            {empty && deviceScope ? null : empty && deviceBrowser === "all" ? (
              <p role="status" className="py-3 text-sm text-[var(--color-text-secondary)]">
                {t("localSessions.empty")}
              </p>
            ) : empty ? (
              <LocalSessionsWelcome
                isDark={isDark}
                onLocalSetup={() => showHelp("setup")}
                onAddDevice={() => navigate("device")}
              />
            ) : (
              <LocalSessionsSelection
                connection={connection}
                isDark={isDark}
                onShowTroubleshooting={() => showHelp("troubleshooting")}
              />
            )}
          </div>
          {view === "group" && <LocalSessionsConfirmation connection={connection} />}
        </div>
      </div>
    </ModalFrame>
  );
}
