import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LocalSessionSource } from "../../types";
import { ActorAvatar } from "../../components/ActorAvatar";
import { LocalSessionsIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";

export function LocalSessionsList({
  sources,
  selected,
  setSelected,
  members,
  membersReady,
  loading,
  error,
  busy,
  isDark,
  limit,
  onResume,
}: {
  sources: LocalSessionSource[];
  selected: LocalSessionSource[];
  setSelected: Dispatch<SetStateAction<LocalSessionSource[]>>;
  members: ReadonlyMap<string, Actor>;
  membersReady: boolean;
  loading: boolean;
  error: string;
  busy: boolean;
  isDark: boolean;
  limit: number;
  onResume: (actor: Actor, source: LocalSessionSource) => Promise<void>;
}) {
  const { t } = useTranslation("layout");
  if (loading) return <p role="status">{t("common:loading")}</p>;
  if (!sources.length)
    return error ? null : (
      <div className="space-y-3 rounded-xl border border-dashed border-[var(--glass-border)] bg-[var(--glass-tab-bg)] px-4 py-5 text-center">
        <LocalSessionsIcon
          size={24}
          className="mx-auto text-[var(--color-text-muted)]"
          aria-hidden="true"
        />
        <div className="space-y-1">
          <p className="text-sm font-medium text-[var(--color-text-primary)]">
            {t("localSessions.empty")}
          </p>
          <p className="mx-auto max-w-sm text-xs leading-relaxed text-[var(--color-text-secondary)]">
            {t("localSessions.emptyHint")}
          </p>
        </div>
      </div>
    );
  const selectedIds = selected.map((source) => source.source_id);
  return (
    <div className="divide-y divide-[var(--glass-border-subtle)]">
      {sources.map((source) => {
        const checked = selectedIds.includes(source.source_id);
        const member = members.get(source.source_id);
        const origin =
          source.runtime === "codex" && (source.origin === "cli" || source.origin === "vscode")
            ? source.origin === "cli"
              ? "CLI"
              : "VS Code"
            : null;
        return (
          <div
            key={source.source_id}
            className="flex min-h-16 flex-wrap items-center gap-3 py-3 sm:flex-nowrap"
          >
            <label className="flex min-w-0 flex-1 basis-full cursor-pointer items-center gap-3 sm:basis-auto">
              <input
                type="checkbox"
                className="h-4 w-4 shrink-0 accent-[var(--color-text-primary)]"
                checked={checked}
                disabled={
                  busy ||
                  !membersReady ||
                  !!member ||
                  (!checked && (!source.connected || selected.length >= limit))
                }
                onChange={(event) =>
                  setSelected((previous) =>
                    event.target.checked
                      ? [...previous, source]
                      : previous.filter((item) => item.source_id !== source.source_id),
                  )
                }
              />
              <ActorAvatar
                runtime={source.runtime}
                title={source.title}
                isDark={isDark}
                sizeClassName="h-8 w-8"
              />
              <span className="min-w-0">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate text-sm font-medium" title={source.title}>
                    {source.title}
                  </span>
                  {origin ? (
                    <span
                      className="shrink-0 rounded border border-[var(--glass-border-subtle)] px-1.5 py-0.5 text-xs text-[var(--color-text-secondary)]"
                      title={t("localSessions.originHint", { origin })}
                    >
                      {origin}
                    </span>
                  ) : null}
                </span>
                <span
                  className="block truncate text-xs text-[var(--color-text-secondary)]"
                  title={source.cwd}
                >
                  {source.cwd}
                </span>
                {!source.connected && (
                  <span
                    className="block text-xs text-amber-700 dark:text-amber-300"
                    data-status-tone="warning"
                  >
                    {t("localSessions.unavailable")}
                  </span>
                )}
                {member ? (
                  <span className="block text-xs text-[var(--color-text-secondary)]">
                    {t(
                      member.enabled === false
                        ? "localSessions.memberPaused"
                        : "localSessions.memberJoined",
                    )}
                  </span>
                ) : null}
              </span>
            </label>
            {member?.enabled === false ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ml-auto max-sm:min-h-11 sm:ml-0"
                disabled={busy}
                onClick={() => void onResume(member, source)}
              >
                {t("actors:dotsResume")}
              </Button>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
