import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LocalSessionSource } from "../../types";
import { ActorAvatar } from "../../components/ActorAvatar";
import { Laptop, Monitor } from "lucide-react";
import { Button } from "../../components/ui/button";
import { LocalSessionOrigin, LocalSessionStatus } from "./LocalSessionOrigin";

export function LocalSessionsList({
  sources,
  selected,
  setSelected,
  members,
  loading,
  busy,
  isDark,
  limit,
  onResume,
}: {
  sources: LocalSessionSource[];
  selected: LocalSessionSource[];
  setSelected: Dispatch<SetStateAction<LocalSessionSource[]>>;
  members: ReadonlyMap<string, Actor>;
  loading: boolean;
  busy: boolean;
  isDark: boolean;
  limit: number;
  onResume: (actor: Actor, source: LocalSessionSource) => Promise<void>;
}) {
  const { t } = useTranslation("layout");
  if (loading)
    return (
      <div role="status" className="space-y-3 py-2">
        <p className="text-sm text-[var(--color-text-secondary)]">{t("common:loading")}</p>
        <div
          aria-hidden="true"
          className="divide-y divide-[var(--glass-border)] border border-[var(--glass-border)]"
        >
          {[0, 1, 2].map((index) => (
            <div key={index} className="flex items-center gap-3 px-4 py-3">
              <span className="h-8 w-8 animate-pulse bg-[var(--glass-tab-bg-hover)] motion-reduce:animate-none" />
              <span className="h-3 flex-1 animate-pulse bg-[var(--glass-tab-bg-hover)] motion-reduce:animate-none" />
            </div>
          ))}
        </div>
      </div>
    );
  if (!sources.length) return null;
  const selectedIds = selected.map((source) => source.source_id);
  const devices = new Map<string, LocalSessionSource[]>();
  for (const source of sources) {
    const device = source.origin?.startsWith("remote:")
      ? source.origin.slice(7)
      : t("localSessions.thisDevice");
    const group = devices.get(device) || [];
    group.push(source);
    devices.set(device, group);
  }
  return (
    <div className="space-y-5">
      {[...devices].map(([device, sessions]) => (
        <section
          key={device}
          aria-label={device}
          className="min-w-0 border border-[var(--glass-border)]"
        >
          <h3 className="flex min-w-0 items-center gap-2 border-b border-[var(--glass-border)] px-4 py-2 text-xs font-semibold uppercase tracking-wider text-[var(--color-text-secondary)]">
            {device === t("localSessions.thisDevice") ? (
              <Monitor size={14} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <Laptop size={14} strokeWidth={1.5} aria-hidden="true" />
            )}
            <span className="min-w-0 flex-1 truncate normal-case tracking-normal" title={device}>
              {device}
            </span>
            <span className="font-normal tabular-nums text-[var(--color-text-muted)]">
              {sessions.length}
            </span>
          </h3>
          <div className="divide-y divide-[var(--glass-border)]">
            {sessions.map((source) => {
              const checked = selectedIds.includes(source.source_id);
              const member = members.get(source.source_id);
              return (
                <div
                  key={source.source_id}
                  className={`flex flex-wrap items-center gap-3 px-4 py-3 transition-colors sm:flex-nowrap ${checked ? "bg-[var(--glass-tab-bg-active)]" : "hover:bg-[var(--glass-tab-bg-hover)]"}`}
                >
                  <label className="flex min-w-0 flex-1 basis-full cursor-pointer items-center gap-3 sm:basis-auto">
                    <input
                      type="checkbox"
                      className="h-4 w-4 shrink-0 accent-[var(--color-text-primary)]"
                      checked={checked}
                      disabled={
                        busy || (!checked && (!source.connected || selected.length >= limit))
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
                    <span className="min-w-0 flex-1 space-y-0.5">
                      <span className="flex min-w-0 items-center gap-3">
                        <span
                          className="min-w-0 flex-1 truncate text-sm font-semibold"
                          title={source.title}
                        >
                          {source.title}
                        </span>
                        <LocalSessionStatus connected={source.connected} />
                      </span>
                      <LocalSessionOrigin source={source} />
                      {member ? (
                        <span className="block text-xs text-[var(--color-text-muted)]">
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
                      className="ml-auto text-sm max-sm:min-h-11 sm:ml-0"
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
        </section>
      ))}
    </div>
  );
}
