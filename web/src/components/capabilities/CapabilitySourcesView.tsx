import { Power, PowerOff, Trash2 } from "lucide-react";
import type React from "react";
import { useTranslation } from "react-i18next";

import type { CapabilitySourceInstance, CapabilitySourceState } from "../../types";
import {
  capabilityCenterSourceRemovalAction,
  capabilityCenterSourcesGridClass,
} from "./capabilityCenterModel";
import { CapabilitySourceInstances } from "./CapabilitySourceInstances";
import { HoverTooltip } from "../HoverTooltip";

function SourceStatusBadge({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation("settings");
  return (
    <span className="inline-flex h-6 w-fit items-center rounded border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2 text-[11px] font-medium text-[var(--color-text-secondary)]">
      {enabled ? t("capabilityCenter.status.enabled") : t("capabilityCenter.status.disabled")}
    </span>
  );
}

function TooltipIconButton({
  label,
  children,
}: {
  label: React.ReactNode;
  children: (referenceProps: Record<string, unknown>, setReference: (node: HTMLElement | null) => void) => React.ReactNode;
}) {
  return (
    <HoverTooltip label={label}>
      {(getReferenceProps, setReference) => children(getReferenceProps({ className: "inline-flex" }), setReference)}
    </HoverTooltip>
  );
}

function sourceSummary(sources: Record<string, CapabilitySourceState>) {
  const entries = Object.values(sources || {});
  const enabled = entries.filter((item) => item.enabled).length;
  return { total: entries.length, enabled, disabled: entries.length - enabled };
}

function SourceStat({ label, value }: { label: string; value: number }) {
  return (
    <div className="min-w-0 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2">
      <div className="truncate text-[11px] text-[var(--color-text-muted)]">{label}</div>
      <div className="text-lg font-semibold tabular-nums sm:text-xl">{value}</div>
    </div>
  );
}

export function SourcesView(props: {
  sources: Record<string, CapabilitySourceState>;
  sourceInstances: CapabilitySourceInstance[];
  busyKey: string;
  onToggle: (sourceId: string, nextEnabled: boolean) => void;
  onDelete: (source: CapabilitySourceState) => void;
  onDeleteInstance: (instance: CapabilitySourceInstance) => void;
}) {
  const { t } = useTranslation("settings");
  const rows = Object.values(props.sources || {}).sort((left, right) => String(left.source_id || "").localeCompare(String(right.source_id || "")));
  return (
    <div className="h-full min-h-0 overflow-auto p-4 lg:p-6">
      <div className="grid gap-2">
        {rows.map((source) => {
          const sourceId = String(source.source_id || "");
          const enabled = Boolean(source.enabled);
          const canDelete = capabilityCenterSourceRemovalAction(source) !== "none";
          const toggleLabel = enabled ? t("capabilityCenter.sources.disable") : t("capabilityCenter.sources.enable");
          const ToggleIcon = enabled ? PowerOff : Power;
          return (
            <div key={sourceId} className={capabilityCenterSourcesGridClass()}>
              <div className="min-w-0">
                <div className="break-words font-medium [overflow-wrap:anywhere] md:truncate">{sourceId}</div>
                <div className="mt-0.5 line-clamp-2 text-xs text-[var(--color-text-muted)] md:truncate">{source.rationale || source.error || t("capabilityCenter.emptyDash")}</div>
              </div>
              <div className="flex justify-end md:block"><SourceStatusBadge enabled={enabled} /></div>
              <span className="col-span-2 text-xs text-[var(--color-text-secondary)] md:col-span-1">{t("capabilityCenter.sources.records", { count: source.record_count || 0 })}</span>
              <span className="col-span-2 truncate text-xs text-[var(--color-text-muted)] md:col-span-1">{t("capabilityCenter.sources.sync", { state: source.sync_state || "unknown" })}</span>
              <div className="col-span-2 flex w-full items-center justify-start gap-2 md:col-span-1 md:justify-end">
                <TooltipIconButton label={toggleLabel}>
                  {(referenceProps, setReference) => (
                    <span ref={setReference} {...referenceProps}>
                      <button
                        type="button"
                        className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-bg-hover)] disabled:opacity-60 md:h-8 md:w-8"
                        disabled={props.busyKey === `source:${sourceId}`}
                        aria-label={toggleLabel}
                        onClick={() => props.onToggle(sourceId, !enabled)}
                      >
                        <ToggleIcon size={14} aria-hidden="true" />
                      </button>
                    </span>
                  )}
                </TooltipIconButton>
                {canDelete ? (
                  <TooltipIconButton label={t("capabilityCenter.sources.delete")}>
                    {(referenceProps, setReference) => (
                      <span ref={setReference} {...referenceProps}>
                        <button
                          type="button"
                          className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-rose-500/30 bg-rose-500/10 text-xs text-rose-600 hover:bg-rose-500/15 disabled:opacity-60 dark:text-rose-300 md:h-8 md:w-8"
                          disabled={props.busyKey === `source-delete:${sourceId}`}
                          aria-label={props.busyKey === `source-delete:${sourceId}` ? t("capabilityCenter.sources.deleting") : t("capabilityCenter.sources.delete")}
                          onClick={() => props.onDelete(source)}
                        >
                          <Trash2 size={14} aria-hidden="true" />
                        </button>
                      </span>
                    )}
                  </TooltipIconButton>
                ) : null}
              </div>
            </div>
          );
        })}
        {rows.length === 0 ? <div className="text-sm text-[var(--color-text-muted)]">{t("capabilityCenter.sources.empty")}</div> : null}
      </div>
      <CapabilitySourceInstances
        instances={props.sourceInstances}
        busyKey={props.busyKey}
        onDelete={props.onDeleteInstance}
      />
    </div>
  );
}

export function SourcesSummary({ sources, sourceInstances }: { sources: Record<string, CapabilitySourceState>; sourceInstances: CapabilitySourceInstance[] }) {
  const { t } = useTranslation("settings");
  const stats = sourceSummary(sources);
  return (
    <aside className="hidden min-h-0 min-w-0 overflow-x-hidden overflow-y-auto bg-[var(--glass-panel-bg)] p-5 lg:block">
      <div className="text-xs uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{t("capabilityCenter.sections.sources.label")}</div>
      <h3 className="mt-1 text-lg font-semibold">{t("capabilityCenter.sections.sources.hint")}</h3>
      <div className="mt-5 grid gap-2">
        <SourceStat label={t("capabilityCenter.sources.total")} value={stats.total} />
        <SourceStat label={t("capabilityCenter.sources.instances")} value={sourceInstances.length} />
        <SourceStat label={t("capabilityCenter.status.enabled")} value={stats.enabled} />
        <SourceStat label={t("capabilityCenter.status.disabled")} value={stats.disabled} />
      </div>
    </aside>
  );
}
