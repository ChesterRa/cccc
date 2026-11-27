import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { AccessTokenEntry } from "../../../services/api";
import { Button } from "../../ui/button";

const PAGE_SIZE = 10;

/** Client-side pages over the same authorized token list; mutations stay with its owner. */
export function WebAccessTokenPages({
  entries,
  children,
}: {
  entries: readonly AccessTokenEntry[];
  children: (entry: AccessTokenEntry) => ReactNode;
}) {
  const { t } = useTranslation("settings");
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(entries.length / PAGE_SIZE));
  const current = Math.min(page, pages - 1);
  const start = current * PAGE_SIZE;

  useEffect(() => {
    // Commit the clamped page so a later create cannot revive a removed last page.
    setPage((previous) => Math.min(previous, pages - 1));
  }, [pages]);

  if (!entries.length) return null;
  return (
    <div className="space-y-3">
      {entries.slice(start, start + PAGE_SIZE).map(children)}
      <nav
        aria-label={t("webAccess.accessControlTitle")}
        className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--glass-border-subtle)] pt-3 text-xs text-[var(--color-text-secondary)]"
      >
        <span role="status">
          {t("capabilityCenter.pagination.showing", {
            from: start + 1,
            to: Math.min(start + PAGE_SIZE, entries.length),
            total: entries.length,
          })}
        </span>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={current === 0}
            onClick={() => setPage(Math.max(0, current - 1))}
          >
            {t("capabilityCenter.pagination.prev")}
          </Button>
          <span className="min-w-12 text-center tabular-nums">
            {current + 1} / {pages}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={current + 1 === pages}
            onClick={() => setPage(Math.min(pages - 1, current + 1))}
          >
            {t("capabilityCenter.pagination.next")}
          </Button>
        </div>
      </nav>
    </div>
  );
}
