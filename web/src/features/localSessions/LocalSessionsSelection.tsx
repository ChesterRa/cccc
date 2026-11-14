import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { LocalSessionsList } from "./LocalSessionsList";
import {
  MAX_SELECTED_SESSIONS,
  type useLocalSessionsConnection,
} from "./useLocalSessionsConnection";

export function LocalSessionsSelection({
  connection,
  isDark,
  onShowTroubleshooting,
}: {
  connection: ReturnType<typeof useLocalSessionsConnection>;
  isDark: boolean;
  onShowTroubleshooting: () => void;
}) {
  const { t } = useTranslation("layout");
  return (
    <section className="space-y-5" aria-label={t("localSessions.sessionsHeading")}>
      {connection.error && (
        <p role="alert" className="break-words text-[var(--color-danger)]">
          {t("localSessions.loadFailed")}
          <span className="block">{connection.error}</span>
        </p>
      )}
      {connection.notice && <p role="status">{connection.notice}</p>}
      {connection.unavailableCount > 0 && (
        <p role="alert" className="text-amber-700 dark:text-amber-300" data-status-tone="warning">
          {t("localSessions.selectionUnavailable", { count: connection.unavailableCount })}
        </p>
      )}
      <LocalSessionsList
        sources={connection.displayedSources}
        selected={connection.selected}
        setSelected={connection.setSelected}
        members={connection.members}
        loading={connection.loading}
        busy={connection.busy}
        isDark={isDark}
        limit={MAX_SELECTED_SESSIONS}
        onResume={connection.resume}
      />
      {!connection.loading && connection.anyUnavailable && (
        <Button variant="ghost" className="min-h-11 text-sm" onClick={onShowTroubleshooting}>
          {t("localSessions.showTroubleshooting")}
        </Button>
      )}
    </section>
  );
}
