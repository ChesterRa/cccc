import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { LocalSessionsList } from "./LocalSessionsList";
import { LocalSessionsDefaults } from "./LocalSessionsDefaults";
import { LocalSessionsHelp, type LocalSessionsHelpRequest } from "./LocalSessionsHelp";
import { MAX_SELECTED_SESSIONS, useLocalSessionsConnection } from "./useLocalSessionsConnection";

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
  const connection = useLocalSessionsConnection({ onClose, onSelectGroup, initialTargetGroupId });
  const [helpRequest, setHelpRequest] = useState<LocalSessionsHelpRequest | null>(null);
  const showHelp = (section: LocalSessionsHelpRequest["section"]) =>
    setHelpRequest((previous) => ({ section, sequence: (previous?.sequence || 0) + 1 }));
  // Setup opens by itself only when the first list that loads is empty; after that the reader
  // decides. A failed load decides nothing, so a later empty list still opens it.
  const [firstListEmpty, setFirstListEmpty] = useState<boolean | null>(null);
  if (firstListEmpty === null && !connection.loading && !connection.error)
    setFirstListEmpty(connection.displayedSources.length === 0);
  return (
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      titleId="local-sessions-title"
      title={t("localSessions.title")}
      closeAriaLabel={t("common:close")}
      modalRef={modalRef}
      panelClassName="w-full sm:max-w-xl sm:max-h-[80dvh]"
      surface="solid"
      footerActions={
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <p role="status" className="text-xs text-[var(--color-text-secondary)]">
            {t("localSessions.selectedCount", {
              count: connection.selected.length,
              limit: MAX_SELECTED_SESSIONS,
            })}
          </p>
          <Button
            disabled={!connection.canSubmit}
            aria-busy={connection.busy}
            onClick={() => void connection.start()}
          >
            {t(
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
      }
    >
      <div
        className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4 sm:p-5"
        aria-busy={connection.loading || connection.busy}
        data-lcd-scope
      >
        <section className="space-y-2" aria-label={t("localSessions.target")}>
          <h3 className="text-xs font-medium text-[var(--color-text-muted)]">
            {t("localSessions.target")}
          </h3>
          <SelectMenu
            ariaLabel={t("localSessions.target")}
            value={connection.targetGroupId}
            onChange={connection.setTargetGroupId}
            disabled={connection.busy}
            placeholder={t("localSessions.targetUnavailable")}
            align="start"
            className="w-full"
            contentClassName="w-[var(--radix-popover-trigger-width)]"
            options={[
              { value: "", label: t("localSessions.newConversation") },
              ...connection.groups.map((group) => ({
                value: group.group_id,
                label: group.title || group.group_id,
              })),
            ]}
          />
          <p className="text-xs text-[var(--color-text-secondary)]">
            {t(connection.joining ? "localSessions.joinHint" : "localSessions.groupHint")}
          </p>
        </section>
        <section className="space-y-3" aria-label={t("localSessions.sessionsHeading")}>
          <div className="space-y-1">
            <h3 className="text-sm font-medium text-[var(--color-text-primary)]">
              {t("localSessions.sessionsHeading")}
            </h3>
            <p className="text-xs text-[var(--color-text-secondary)]">{t("localSessions.hint")}</p>
          </div>
          {connection.error && (
            <p role="alert" className="text-sm text-red-600" data-status-tone="error">
              {t("localSessions.loadFailed")}
              <span className="block text-xs">{connection.error}</span>
            </p>
          )}
          {connection.targetUnavailable && (
            <p role="alert" className="text-sm text-red-600" data-status-tone="error">
              {t("localSessions.targetUnavailable")}
            </p>
          )}
          {connection.joining && !connection.membersReady ? (
            <p role={connection.membership.failed ? "alert" : "status"} className="text-sm">
              {t(connection.membership.failed ? "localSessions.membersFailed" : "common:loading")}
            </p>
          ) : null}
          {connection.notice ? (
            <p role="status" className="text-sm">
              {connection.notice}
            </p>
          ) : null}
          {connection.unavailableCount > 0 && (
            <p
              role="alert"
              className="text-sm text-amber-700 dark:text-amber-300"
              data-status-tone="warning"
            >
              {t("localSessions.selectionUnavailable", { count: connection.unavailableCount })}
            </p>
          )}
          {!connection.loading && connection.anyUnavailable ? (
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t("localSessions.unavailableHint")}{" "}
              <button
                type="button"
                className="underline underline-offset-2"
                onClick={() => showHelp("troubleshooting")}
              >
                {t("localSessions.showTroubleshooting")}
              </button>
            </p>
          ) : null}
          <LocalSessionsList
            sources={connection.displayedSources}
            selected={connection.selected}
            setSelected={connection.setSelected}
            members={connection.members}
            membersReady={connection.membersReady}
            loading={connection.loading}
            error={connection.error}
            busy={connection.busy}
            isDark={isDark}
            limit={MAX_SELECTED_SESSIONS}
            onResume={connection.resume}
          />
        </section>
        {!connection.joining && (
          <LocalSessionsDefaults
            selected={connection.selected}
            setSelected={connection.setSelected}
            busy={connection.busy}
          />
        )}
        <LocalSessionsHelp request={helpRequest} openSetup={firstListEmpty === true} />
      </div>
    </ModalFrame>
  );
}
