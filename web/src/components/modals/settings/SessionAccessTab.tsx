import { useTranslation } from "react-i18next";
import {
  LocalSessionOrigin,
  LocalSessionStatus,
} from "../../../features/localSessions/LocalSessionOrigin";
import { LocalSessionsHelp } from "../../../features/localSessions/LocalSessionsHelp";
import { LocalSourceActions } from "../../../features/localSessions/LocalSourceActions";
import { LocalSourceDiagnostic } from "../../../features/localSessions/LocalSourceDiagnostic";
import { LocalSourceManagement } from "../../../features/localSessions/LocalSourceManagement";
import { SessionNodesPanel } from "../../../features/localSessions/SessionNodesPanel";
import { useLocalSessionSources } from "../../../features/localSessions/useLocalSessionSources";
import { useLocalSourceControls } from "../../../features/localSessions/useLocalSourceControls";

/**
 * Instance-wide management of which running sessions may join groups. Joining itself happens in a
 * group's Add Agent; this page only manages this computer's sources and the paired computers.
 */
export function SessionAccessTab({ isActive }: { isActive: boolean }) {
  const { t } = useTranslation("layout");
  const directory = useLocalSessionSources({ active: isActive, includeDeviceSources: false });
  const management = useLocalSourceControls(directory);
  const busy = management.busy;
  return (
    <div className="min-w-0 space-y-5">
      <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
        {t("sessionAccess.intro")}
      </p>
      <section aria-labelledby="session-access-local" className="min-w-0 space-y-3">
        <h3
          id="session-access-local"
          tabIndex={-1}
          data-source-control-return
          className="text-base font-semibold focus:outline-none"
        >
          {t("sessionAccess.thisComputer")}
        </h3>
        {directory.loading && !directory.sources.length ? (
          <p role="status" className="text-sm text-[var(--color-text-secondary)]">
            {t("common:loading")}
          </p>
        ) : directory.error ? (
          <p role="alert" className="break-words text-sm text-[var(--color-accent-danger)]">
            {directory.error}
          </p>
        ) : directory.sources.length ? (
          <ul className="divide-y divide-[var(--glass-border-subtle)] border-y border-[var(--glass-border-subtle)]">
            {directory.sources.map((source) => (
              <li
                key={source.source_id}
                className="flex min-w-0 flex-wrap items-start justify-between gap-2 py-3"
              >
                <div className="min-w-0 flex-1 space-y-1">
                  <p className="flex min-w-0 flex-wrap items-center gap-x-2 break-words text-sm font-semibold [overflow-wrap:anywhere]">
                    {source.title}
                    {!source.connected && <LocalSessionStatus connected={false} />}
                  </p>
                  <LocalSessionOrigin source={source} />
                  <LocalSourceDiagnostic
                    state={directory.source_states?.[source.source_id]}
                    supported={!!directory.capabilities?.source_diagnostics}
                  />
                </div>
                <LocalSourceActions
                  source={source}
                  directory={directory}
                  management={management}
                  busy={busy}
                />
              </li>
            ))}
          </ul>
        ) : (
          <p role="status" className="text-sm text-[var(--color-text-secondary)]">
            {t("sessionAccess.noLocalSessions")}
          </p>
        )}
        <LocalSourceManagement directory={directory} management={management} busy={busy} />
        <LocalSessionsHelp onChooseSessions={() => void directory.reload()} />
      </section>
      <section
        aria-labelledby="session-access-computers"
        className="min-w-0 space-y-3 border-t border-[var(--glass-border-subtle)] pt-4"
      >
        <h3 id="session-access-computers" className="text-base font-semibold">
          {t("sessionAccess.otherComputers")}
        </h3>
        <SessionNodesPanel mode="settings" active={isActive} />
      </section>
    </div>
  );
}
