import { Shield } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "../../lib/utils";
import type { CapabilityOverviewItem } from "../../types";
import { LcdIcon } from "../Icons";
import { capabilityCenterDisplayName, capabilityCenterTypeLabel } from "./capabilityCenterModel";
import {
  RECOMMENDATION_LEGEND,
  recommendationDate,
  recommendationDot,
  recommendationEntry,
  recommendationVerification,
  recommendationVerificationKey,
  type RecommendationCell,
  type RecommendationDot,
  type RecommendationRow,
} from "./recommendationModel";

/** The Group state of one capability, as the legend explains it; words go in the hover tip. */
export function RecommendationDotIcon({
  dot,
  className,
}: {
  dot: RecommendationDot;
  className?: string;
}) {
  const { t } = useTranslation("settings");
  const label = t(`capabilityCenter.recommended.dot.${dot}`);
  if (dot === "none") {
    return <span className={cn("inline-block h-3 w-3 shrink-0", className)} title={label} />;
  }
  if (dot === "blocked") {
    return (
      <span
        className={cn(
          "inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center text-rose-700 dark:text-rose-300",
          className,
        )}
        data-status-tone="error"
        role="img"
        aria-label={label}
        title={label}
      >
        <LcdIcon icon={Shield} glyph="shield" size={13} aria-hidden="true" />
      </span>
    );
  }
  const ring = (
    <circle
      cx="6"
      cy="6"
      r="4.75"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeDasharray={dot === "unknown" || dot === "pending" ? "2 1.7" : undefined}
    />
  );
  return (
    <span
      className={cn(
        "inline-flex h-3 w-3 shrink-0",
        dot === "setup" ? "text-amber-700 dark:text-amber-300" : "",
        className,
      )}
      data-status-tone={dot === "setup" ? "warning" : undefined}
      role="img"
      aria-label={label}
      title={label}
    >
      <svg viewBox="0 0 12 12" className="h-3 w-3" aria-hidden="true">
        {dot === "group" ? <circle cx="6" cy="6" r="5" fill="currentColor" /> : ring}
        {dot === "member" ? <circle cx="6" cy="6" r="2" fill="currentColor" /> : null}
        {dot === "setup" ? (
          <path d="M6 3.4v3.2M6 8.2v.5" stroke="currentColor" strokeWidth="1.5" />
        ) : null}
        {dot === "unsupported" ? (
          <path d="M2.7 9.3 9.3 2.7" stroke="currentColor" strokeWidth="1.5" />
        ) : null}
      </svg>
    </span>
  );
}

/** How far it was checked; a failed check reads in the waiting colour. */
export function RecommendationVerificationLabel({
  item,
  className,
}: {
  item: Pick<CapabilityOverviewItem, "verification" | "support">;
  className?: string;
}) {
  const { t } = useTranslation("settings");
  const top = recommendationVerification(item.verification);
  const failed = top?.outcome === "failed";
  const hint =
    top?.level === "listed" && !failed
      ? t("capabilityCenter.recommended.verification.listedHint")
      : [recommendationDate(top?.observed_at), top?.result].filter(Boolean).join(" · ");
  return (
    <span
      className={cn(
        "shrink-0 text-[11px] leading-5",
        failed ? "text-amber-700 dark:text-amber-300" : "text-[var(--color-text-muted)]",
        className,
      )}
      data-status-tone={failed ? "warning" : undefined}
      title={hint || undefined}
    >
      {t(
        `capabilityCenter.recommended.verification.${recommendationVerificationKey(top, item.support?.form)}`,
      )}
    </span>
  );
}

function RecommendationItem(props: {
  item: CapabilityOverviewItem;
  domainId: string;
  alternative: boolean;
  kindLabel?: string;
  selected: boolean;
  /** False when the selection is only the wide screen's default, which a phone does not show. */
  explicit: boolean;
  onSelect: (capabilityId: string) => void;
}) {
  const { t } = useTranslation("settings");
  const capId = String(props.item.capability_id || "");
  const reason = props.alternative
    ? ""
    : String(recommendationEntry(props.item, props.domainId)?.reason || "").trim();
  return (
    <button
      type="button"
      className={cn(
        "grid w-full min-w-0 gap-0.5 border-l-2 px-3 py-2 text-left text-sm",
        props.selected && props.explicit
          ? "border-[var(--color-text-primary)] bg-[var(--glass-bg-active)]"
          : props.selected
            ? "border-transparent hover:bg-[var(--glass-bg-hover)] lg:border-[var(--color-text-primary)] lg:bg-[var(--glass-bg-active)]"
            : "border-transparent hover:bg-[var(--glass-bg-hover)]",
        props.kindLabel ? "grid-cols-[3rem_minmax(0,1fr)]" : "",
      )}
      aria-current={props.selected ? "true" : undefined}
      onClick={() => props.onSelect(capId)}
    >
      {props.kindLabel ? (
        <span className="row-span-2 pt-px text-xs text-[var(--color-text-muted)]">
          {props.kindLabel}
        </span>
      ) : null}
      {/* In a narrow cell the labels after the name drop to their own line instead of
          squeezing the name until words break in the middle. */}
      <span className="flex min-w-0 flex-wrap items-start gap-x-2">
        <span className="flex min-w-0 flex-[1_1_auto] items-start gap-2">
          <RecommendationDotIcon dot={recommendationDot(props.item)} className="mt-1" />
          <span className="min-w-0 break-words font-medium [overflow-wrap:anywhere]">
            {props.alternative ? (
              <>
                <span className="mr-1.5 text-[11px] font-normal text-[var(--color-text-muted)]">
                  {t("capabilityCenter.recommended.alternative")}
                </span>
                {/* A break opportunity: without it a Latin label and name form one word. */}
                <wbr />
              </>
            ) : null}
            {capabilityCenterDisplayName(props.item)}
          </span>
        </span>
        <span className="ml-auto flex shrink-0 items-start gap-2">
          {props.item.update?.available && props.item.update.reason === "recipe_changed" ? (
            <span
              className="text-[11px] leading-5 text-amber-700 dark:text-amber-300"
              data-status-tone="warning"
            >
              {t("capabilityCenter.update.badge")}
            </span>
          ) : null}
          <RecommendationVerificationLabel item={props.item} />
        </span>
      </span>
      {reason ? (
        <span className="line-clamp-2 pl-5 text-xs leading-5 text-[var(--color-text-secondary)]">
          {reason}
        </span>
      ) : null}
    </button>
  );
}

function CellItems(props: {
  cell: RecommendationCell;
  domainId: string;
  kindLabel?: string;
  selectedId: string;
  explicit: boolean;
  onSelect: (capabilityId: string) => void;
}) {
  const items = [
    ...(props.cell.primary ? [{ item: props.cell.primary, alternative: false }] : []),
    ...props.cell.alternatives.map((item) => ({ item, alternative: true })),
  ];
  return items.map(({ item, alternative }) => (
    <RecommendationItem
      key={String(item.capability_id)}
      item={item}
      domainId={props.domainId}
      alternative={alternative}
      kindLabel={props.kindLabel}
      selected={String(item.capability_id) === props.selectedId}
      explicit={props.explicit}
      onSelect={props.onSelect}
    />
  ));
}

function EmptyCell() {
  const { t } = useTranslation("settings");
  return (
    <span
      className="block px-3 py-2 text-sm text-[var(--color-text-muted)]"
      title={t("capabilityCenter.recommended.emptyCell")}
    >
      {t("capabilityCenter.emptyDash")}
    </span>
  );
}

export function CapabilityRecommendedView(props: {
  rows: RecommendationRow[];
  groupTitle: string;
  loading: boolean;
  selectedId: string;
  /** Whether the user picked the selection, or it is the wide screen's first example. */
  explicit: boolean;
  onSelect: (capabilityId: string) => void;
}) {
  const { t } = useTranslation("settings");
  const hasItems = props.rows.some(
    (row) =>
      row.mcp.primary ||
      row.skill.primary ||
      row.mcp.alternatives.length ||
      row.skill.alternatives.length,
  );
  return (
    <div
      className="@container/recommended h-full min-h-0 overflow-auto scrollbar-subtle"
      data-capability-recommended
    >
      <div className="grid gap-1 px-3 py-3 text-sm text-[var(--color-text-secondary)] sm:px-4 lg:px-6">
        <p>{t("capabilityCenter.recommended.intro")}</p>
        {props.groupTitle ? (
          <p className="text-xs text-[var(--color-text-muted)] sm:hidden">
            {t("capabilityCenter.scopeGroup", { name: props.groupTitle })}
          </p>
        ) : null}
      </div>
      {!hasItems ? (
        <div className="px-6 py-10 text-sm text-[var(--color-text-muted)]">
          {props.loading ? t("capabilityCenter.loading") : t("capabilityCenter.recommended.empty")}
        </div>
      ) : (
        <>
          {/* Wide: one row per area, the MCP and the Skill side by side. */}
          <div className="hidden px-3 sm:px-4 lg:px-6 @min-[40rem]/recommended:block">
            <div className="grid grid-cols-[minmax(7rem,0.7fr)_minmax(0,1fr)_minmax(0,1fr)] border-b border-[var(--glass-border-subtle)] py-2 text-xs font-semibold text-[var(--color-text-muted)]">
              <div className="px-1">{t("capabilityCenter.recommended.domain")}</div>
              <div className="px-3">{t("capabilityCenter.recommended.mcp")}</div>
              <div className="px-3">{t("capabilityCenter.recommended.skill")}</div>
            </div>
            <div className="divide-y divide-[var(--glass-border-subtle)]">
              {props.rows.map((row) => (
                <div
                  key={row.domain.domain_id}
                  className="grid grid-cols-[minmax(7rem,0.7fr)_minmax(0,1fr)_minmax(0,1fr)] items-start py-1"
                >
                  <div className="px-1 py-2 text-sm text-[var(--color-text-secondary)]">
                    {row.domain.name}
                  </div>
                  {(["mcp", "skill"] as const).map((kind) =>
                    row[kind].primary || row[kind].alternatives.length ? (
                      <div key={kind} className="min-w-0">
                        <CellItems
                          cell={row[kind]}
                          domainId={row.domain.domain_id}
                          selectedId={props.selectedId}
                          explicit={props.explicit}
                          onSelect={props.onSelect}
                        />
                      </div>
                    ) : (
                      <EmptyCell key={kind} />
                    ),
                  )}
                </div>
              ))}
            </div>
          </div>
          {/* Narrow: one section per area; empty places are left out. */}
          <div className="divide-y divide-[var(--glass-border-subtle)] px-3 sm:px-4 @min-[40rem]/recommended:hidden">
            {props.rows.map((row) => (
              <section key={row.domain.domain_id} className="py-2">
                <h3 className="px-1 py-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
                  {row.domain.name}
                </h3>
                {row.mcp.primary ||
                row.skill.primary ||
                row.mcp.alternatives.length ||
                row.skill.alternatives.length ? (
                  (["mcp", "skill"] as const).map((kind) => (
                    <CellItems
                      key={kind}
                      cell={row[kind]}
                      domainId={row.domain.domain_id}
                      kindLabel={capabilityCenterTypeLabel(kind)}
                      selectedId={props.selectedId}
                      explicit={props.explicit}
                      onSelect={props.onSelect}
                    />
                  ))
                ) : (
                  <EmptyCell />
                )}
              </section>
            ))}
          </div>
          <RecommendationLegend />
        </>
      )}
    </div>
  );
}

function RecommendationLegend() {
  const { t } = useTranslation("settings");
  return (
    <section
      className="mx-3 mb-4 mt-3 border-t border-[var(--glass-border-subtle)] pt-3 sm:mx-4 lg:mx-6"
      aria-label={t("capabilityCenter.recommended.legend")}
    >
      <ul className="flex flex-wrap gap-x-4 gap-y-1.5 text-xs text-[var(--color-text-muted)]">
        {RECOMMENDATION_LEGEND.map((dot) => (
          <li key={dot} className="inline-flex items-center gap-1.5">
            <RecommendationDotIcon dot={dot} />
            <span>
              {t(`capabilityCenter.recommended.dot.${dot === "unknown" ? "uncertain" : dot}`)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
