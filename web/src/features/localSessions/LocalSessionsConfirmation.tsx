import { useTranslation } from "react-i18next";
import { SelectMenu } from "../../components/ui/select-menu";
import { Button } from "../../components/ui/button";
import { LocalSessionOrigin } from "./LocalSessionOrigin";
import { LocalSessionsDefaults } from "./LocalSessionsDefaults";
import type { useLocalSessionsConnection } from "./useLocalSessionsConnection";

export function LocalSessionsConfirmation({
  connection,
}: {
  connection: ReturnType<typeof useLocalSessionsConnection>;
}) {
  const { t } = useTranslation("layout");
  return (
    <div className="mx-auto w-full min-w-0 max-w-xl space-y-4">
      <div className="min-w-0 space-y-3">
        <section
          className="space-y-3 rounded-xl border border-[var(--glass-border-subtle)] p-3 sm:p-4"
          aria-label={t("localSessions.target")}
        >
          <h3 className="text-sm font-semibold">{t("localSessions.target")}</h3>
          {connection.groups.length > 0 && (
            <SelectMenu
              ariaLabel={t("localSessions.target")}
              value={connection.targetGroupId ?? ""}
              onChange={connection.setTargetGroupId}
              disabled={connection.busy}
              align="start"
              placeholder={t(
                connection.targetUnavailable
                  ? "localSessions.targetUnavailable"
                  : "localSessions.chooseTarget",
              )}
              className="min-h-11 w-full text-sm"
              contentClassName="w-[var(--radix-popover-trigger-width)] [&_[role=menuitemradio]]:text-sm"
              options={connection.groups.map((group) => ({
                value: group.group_id,
                label: group.title || group.group_id,
              }))}
            />
          )}
          <Button
            type="button"
            variant="outline"
            aria-pressed={connection.creatingGroup}
            className="min-h-11 whitespace-normal text-sm aria-pressed:border-[var(--color-border-focus)] aria-pressed:bg-[var(--glass-tab-bg-active)]"
            disabled={connection.busy}
            onClick={() => connection.setTargetGroupId("")}
          >
            {t("localSessions.newConversation")}
          </Button>
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t(
              connection.targetPending
                ? "localSessions.chooseTargetHint"
                : connection.creatingGroup
                  ? "localSessions.groupHint"
                  : "localSessions.joinHint",
            )}
          </p>
        </section>
        {connection.targetUnavailable && (
          <p role="alert" className="text-[var(--color-accent-danger)]">
            {t("localSessions.targetUnavailable")}
          </p>
        )}
        {connection.joining && !connection.membersReady && (
          <p role={connection.membership.failed ? "alert" : "status"}>
            {t(connection.membership.failed ? "localSessions.membersFailed" : "common:loading")}
          </p>
        )}
        {connection.error && (
          <p role="alert" className="text-[var(--color-accent-danger)]">
            {t("localSessions.loadFailed")}
          </p>
        )}
        {connection.unavailableCount > 0 && (
          <p role="alert" className="text-amber-700 dark:text-amber-300" data-status-tone="warning">
            {t("localSessions.selectionUnavailable", { count: connection.unavailableCount })}
          </p>
        )}
        {connection.creatingGroup && (
          <LocalSessionsDefaults
            selected={connection.selected}
            setSelected={connection.setSelected}
            busy={connection.busy}
          />
        )}
      </div>
      <section
        className="min-w-0 space-y-3 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-3 sm:p-4"
        aria-label={t("localSessions.selectedSessions")}
      >
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-semibold">{t("localSessions.selectedSessions")}</h3>
          <p role="status" className="text-xs tabular-nums text-[var(--color-text-secondary)]">
            <span aria-hidden="true">
              {connection.selected.length} / {connection.selectionLimit}
            </span>
            <span className="sr-only">
              {t("localSessions.selectedCount", {
                count: connection.selected.length,
                limit: connection.selectionLimit,
              })}
            </span>
          </p>
        </div>
        {!connection.selected.length && <p role="status">{t("localSessions.noSelection")}</p>}
        <ul className="divide-y divide-[var(--glass-border-subtle)]">
          {connection.selected.map((source) => (
            <li key={source.source_id} className="min-w-0 space-y-1 py-2 first:pt-0 last:pb-0">
              <p dir="auto" className="break-words font-medium [overflow-wrap:anywhere]">
                {source.title}
              </p>
              <LocalSessionOrigin source={source} showDevice />
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
