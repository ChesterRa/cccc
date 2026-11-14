import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { Button } from "../../components/ui/button";
import type { LocalSessionsHelpRequest } from "./LocalSessionsHelp";
import { LocalSessionsGuide } from "./LocalSessionsGuide";
import { LocalSessionsSelection } from "./LocalSessionsSelection";
import { LocalSessionsWelcome } from "./LocalSessionsWelcome";
import { LocalSessionsConfirmation } from "./LocalSessionsConfirmation";
import { MAX_SELECTED_SESSIONS, useLocalSessionsConnection } from "./useLocalSessionsConnection";
import { useSessionDialogNavigation } from "./useSessionDialogNavigation";

export function LocalSessionsDialog({
  isDark,
  onClose,
  onSelectGroup,
  initialTargetGroupId,
}: {
  isDark: boolean;
  onClose: () => void;
  onSelectGroup: (id: string) => void;
  initialTargetGroupId?: string;
}) {
  const { t } = useTranslation("layout");
  const { modalRef } = useModalA11y(true, onClose);
  const navigation = useSessionDialogNavigation();
  const { view, navigate } = navigation;
  const connection = useLocalSessionsConnection({
    onClose,
    onSelectGroup,
    initialTargetGroupId,
    reviewing: view === "group",
  });
  const hasSources = connection.displayedSources.length > 0;
  const empty = !connection.loading && !connection.error && !hasSources;
  const panelWidth =
    view === "sessions" && !hasSources
      ? "sm:max-w-2xl"
      : view === "device" || view === "help"
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
    connection.selected.length <= MAX_SELECTED_SESSIONS &&
    connection.unavailableCount === 0;
  const title = t(
    view === "sessions"
      ? "localSessions.overviewTitle"
      : view === "group"
        ? "localSessions.chooseGroup"
        : view === "device"
          ? "localSessions.addDevice"
          : "localSessions.connectionHelp",
  );
  return (
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      titleId="local-sessions-title"
      title={
        <h2 ref={navigation.titleRef} tabIndex={-1} className="text-lg font-semibold outline-none">
          {title}
        </h2>
      }
      closeAriaLabel={t("common:close")}
      modalRef={modalRef}
      panelClassName={
        view === "sessions" && !hasSources
          ? "self-center h-auto max-h-[88dvh] w-full max-w-[calc(100vw_-_2rem)] sm:max-w-2xl"
          : `h-[100dvh] w-full sm:h-auto sm:max-h-[88dvh] ${panelWidth}`
      }
      surface="solid"
      headerActions={
        view !== "sessions" && (
          <Button
            variant="ghost"
            size="sm"
            className="text-sm max-sm:min-h-11"
            aria-label={t("localSessions.backToSessions")}
            onClick={() => navigate("sessions")}
          >
            <ArrowLeft size={16} aria-hidden="true" />
            <span className="hidden sm:inline">{t("localSessions.backToSessions")}</span>
          </Button>
        )
      }
      footerActions={
        ((view === "sessions" && hasSources) || view === "group") && (
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <p role="status" className="text-sm text-[var(--color-text-secondary)]">
              {t("localSessions.selectedCount", {
                count: connection.selected.length,
                limit: MAX_SELECTED_SESSIONS,
              })}
            </p>
            <Button
              disabled={view === "sessions" ? !canContinue : !connection.canSubmit}
              aria-busy={connection.busy}
              onClick={() => (view === "sessions" ? navigate("group") : void connection.start())}
            >
              {view === "sessions"
                ? t("localSessions.continue")
                : t(
                    connection.joining
                      ? connection.busy
                        ? "localSessions.joining"
                        : "localSessions.join"
                      : connection.busy
                        ? "localSessions.creating"
                        : "localSessions.create",
                  )}
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
        <div className="space-y-5 p-4 sm:p-6">
          <LocalSessionsGuide
            view={view}
            showEntryActions={hasSources || !!connection.error}
            request={helpRequest}
            onAddDevice={() => navigate("device")}
            onShowHelp={() => showHelp("setup")}
            onChooseSessions={() => navigate("sessions")}
          />
          <div hidden={view !== "sessions"}>
            {empty ? (
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
