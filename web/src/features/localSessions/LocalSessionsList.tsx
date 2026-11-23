import { useTranslation } from "react-i18next";
import type { LocalSessionSource } from "../../types";
import { ActorAvatar } from "../../components/ActorAvatar";
import { Button } from "../../components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import { LocalSessionOrigin, LocalSessionStatus } from "./LocalSessionOrigin";
import { LocalSourceActions } from "./LocalSourceActions";
import { LocalSourceDiagnostic } from "./LocalSourceDiagnostic";
import { isDeviceSessionSource } from "./remoteSessionPolicy";
import {
  MAX_SELECTED_SESSIONS,
  type useLocalSessionsConnection,
} from "./useLocalSessionsConnection";

export function LocalSessionsList({
  sources,
  connection,
  isDark,
}: {
  sources: LocalSessionSource[];
  connection: ReturnType<typeof useLocalSessionsConnection>;
  isDark: boolean;
}) {
  const { t } = useTranslation("layout");
  const { busy, directory, management, members, selected, setSelected } = connection;
  const selectedIds = new Set(selected.map((source) => source.source_id));
  return (
    <div className="divide-y divide-[var(--glass-border)]">
      {sources.map((source) => {
        const member = members.get(source.source_id);
        const checked = selectedIds.has(source.source_id);
        const local = !isDeviceSessionSource(source);
        const state = directory.source_states?.[source.source_id];
        const diagnostics = local && Boolean(directory.capabilities?.source_diagnostics);
        const showReason =
          !source.connected || (diagnostics && state?.diagnostic.state !== "ready");
        const disabled =
          busy ||
          (!checked &&
            Boolean(
              member ||
              !source.connected ||
              selected.length >= MAX_SELECTED_SESSIONS ||
              (local && (directory.loading || directory.error)),
            ));
        const diagnostic = <LocalSourceDiagnostic state={state} supported={diagnostics} />;
        return (
          <div
            key={source.source_id}
            className={`space-y-2 px-3 py-2.5 ${checked ? "bg-[var(--glass-tab-bg-active)]" : ""}`}
          >
            <div className="flex min-w-0 items-center gap-2">
              <label className="flex min-w-0 flex-1 items-center gap-3">
                <input
                  type="checkbox"
                  className="h-4 w-4 shrink-0 accent-[var(--color-text-primary)]"
                  checked={checked}
                  disabled={disabled}
                  onChange={(event) => {
                    const next = event.target.checked;
                    setSelected((current) =>
                      next
                        ? [...current, source]
                        : current.filter((item) => item.source_id !== source.source_id),
                    );
                  }}
                />
                <ActorAvatar
                  runtime={source.runtime}
                  title={source.title}
                  isDark={isDark}
                  sizeClassName="h-8 w-8"
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-semibold" title={source.title}>
                    {source.title}
                  </span>
                  <LocalSessionOrigin source={source} />
                  {member && (
                    <span className="block text-xs text-[var(--color-text-muted)]">
                      {t(
                        member.enabled === false
                          ? "localSessions.memberPaused"
                          : "localSessions.memberJoined",
                      )}
                    </span>
                  )}
                </span>
              </label>
              {diagnostics && !showReason ? (
                <Popover>
                  <PopoverTrigger
                    className="flex min-h-11 min-w-9 shrink-0 items-center justify-center"
                    aria-label={t("sourceControls.showReceiveStatus", { title: source.title })}
                  >
                    <LocalSessionStatus connected compact />
                  </PopoverTrigger>
                  <PopoverContent align="end" className="w-52 p-3">
                    {diagnostic}
                  </PopoverContent>
                </Popover>
              ) : (
                <LocalSessionStatus connected={source.connected} compact={source.connected} />
              )}
            </div>
            {showReason && diagnostic}
            {local && showReason && (
              <LocalSourceActions
                source={source}
                directory={directory}
                management={management}
                busy={busy}
                recheckOnly
              />
            )}
            {member?.enabled === false && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="max-sm:min-h-11"
                disabled={
                  busy || !source.connected || (local && (!!directory.error || directory.loading))
                }
                onClick={() => void connection.resume(member, source)}
              >
                {t("localSessions.resumeGroupAccess")}
              </Button>
            )}
          </div>
        );
      })}
    </div>
  );
}
