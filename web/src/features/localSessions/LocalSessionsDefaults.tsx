import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { LocalSessionSource } from "../../types";
import { SelectMenu } from "../../components/ui/select-menu";

export function LocalSessionsDefaults({
  selected,
  setSelected,
  busy,
}: {
  selected: LocalSessionSource[];
  setSelected: Dispatch<SetStateAction<LocalSessionSource[]>>;
  busy: boolean;
}) {
  const { t } = useTranslation("layout");
  const firstSelected = selected[0];
  if (!firstSelected) return null;
  return (
    <section
      className="space-y-2 rounded-xl border border-[var(--glass-border-subtle)] p-3 text-xs text-[var(--color-text-secondary)]"
      aria-label={t("localSessions.newGroupDefaults")}
    >
      <h3 className="font-medium text-[var(--color-text-primary)]">
        {t("localSessions.newGroupDefaults")}
      </h3>
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
        <dt className="self-center">{t("actors:groupRoleForeman")}</dt>
        <dd className="min-w-0 break-words">
          {selected.length > 1 ? (
            <SelectMenu
              ariaLabel={t("actors:groupRoleForeman")}
              value={firstSelected.source_id}
              options={selected.map((source) => ({ value: source.source_id, label: source.title }))}
              onChange={(sourceId) =>
                setSelected((previous) => {
                  const foreman = previous.find((source) => source.source_id === sourceId);
                  return foreman
                    ? [foreman, ...previous.filter((source) => source.source_id !== sourceId)]
                    : previous;
                })
              }
              disabled={busy}
              align="start"
              className="w-full"
              contentClassName="w-[var(--radix-popover-trigger-width)]"
            />
          ) : (
            firstSelected.title
          )}
        </dd>
        <dt>{t("actors:workingDirectory")}</dt>
        <dd className="break-all">{firstSelected.cwd}</dd>
      </dl>
      {selected.length > 1 ? <p>{t("localSessions.foremanDirectoryHint")}</p> : null}
    </section>
  );
}
