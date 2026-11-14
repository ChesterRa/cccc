import { useTranslation } from "react-i18next";
import { SelectMenu } from "../../components/ui/select-menu";
import { LocalSessionsDefaults } from "./LocalSessionsDefaults";
import type { useLocalSessionsConnection } from "./useLocalSessionsConnection";

export function LocalSessionsConfirmation({
  connection,
}: {
  connection: ReturnType<typeof useLocalSessionsConnection>;
}) {
  const { t } = useTranslation("layout");
  return (
    <div className="grid min-w-0 gap-6 md:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]">
      <div className="min-w-0 space-y-5">
        <section className="space-y-3" aria-label={t("localSessions.target")}>
          <h3 className="text-sm font-semibold">{t("localSessions.target")}</h3>
          <SelectMenu
            ariaLabel={t("localSessions.target")}
            value={connection.targetGroupId}
            onChange={connection.setTargetGroupId}
            disabled={connection.busy}
            align="start"
            placeholder={t("localSessions.targetUnavailable")}
            className="min-h-11 w-full text-sm"
            contentClassName="w-[var(--radix-popover-trigger-width)] [&_[role=menuitemradio]]:text-sm"
            options={[
              { value: "", label: t("localSessions.newConversation") },
              ...connection.groups.map((group) => ({
                value: group.group_id,
                label: group.title || group.group_id,
              })),
            ]}
          />
        </section>
        {connection.targetUnavailable && (
          <p role="alert" className="text-[var(--color-danger)]">
            {t("localSessions.targetUnavailable")}
          </p>
        )}
        {connection.joining && !connection.membersReady && (
          <p role={connection.membership.failed ? "alert" : "status"}>
            {t(connection.membership.failed ? "localSessions.membersFailed" : "common:loading")}
          </p>
        )}
        {connection.error && (
          <p role="alert" className="text-[var(--color-danger)]">
            {t("localSessions.loadFailed")}
          </p>
        )}
        {connection.unavailableCount > 0 && (
          <p role="alert" className="text-amber-700 dark:text-amber-300" data-status-tone="warning">
            {t("localSessions.selectionUnavailable", { count: connection.unavailableCount })}
          </p>
        )}
        {!connection.joining && (
          <LocalSessionsDefaults
            selected={connection.selected}
            setSelected={connection.setSelected}
            busy={connection.busy}
          />
        )}
      </div>
      <section
        className="min-w-0 space-y-4 self-start rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-5"
        aria-label={t("localSessions.selectedSessions")}
      >
        <h3 className="text-sm font-semibold">{t("localSessions.selectedSessions")}</h3>
        {!connection.selected.length && <p role="status">{t("localSessions.noSelection")}</p>}
        <ul className="divide-y divide-[var(--glass-border-subtle)]">
          {connection.selected.map((source) => (
            <li key={source.source_id} className="break-words py-3 first:pt-0 last:pb-0">
              {source.title}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
