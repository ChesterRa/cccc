import { Trash2 } from "lucide-react";
import type React from "react";
import { useTranslation } from "react-i18next";

import type { CapabilitySourceInstance, CapabilitySourceState } from "../../types";
import {
  capabilityCenterSourceRemovalAction,
  capabilityCenterSourcesGridClass,
} from "./capabilityCenterModel";
import { CapabilitySourceInstances } from "./CapabilitySourceInstances";
import { HoverTooltip } from "../HoverTooltip";
import { LcdIcon } from "../Icons";

function TooltipIconButton({
  label,
  children,
}: {
  label: React.ReactNode;
  children: (
    referenceProps: Record<string, unknown>,
    setReference: (node: HTMLElement | null) => void,
  ) => React.ReactNode;
}) {
  return (
    <HoverTooltip label={label}>
      {(getReferenceProps, setReference) =>
        children(getReferenceProps({ className: "inline-flex" }), setReference)
      }
    </HoverTooltip>
  );
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
  onDelete: (source: CapabilitySourceState) => void;
  onDeleteInstance: (instance: CapabilitySourceInstance) => void;
}) {
  const { t } = useTranslation("settings");
  const rows = Object.values(props.sources || {}).sort((left, right) =>
    String(left.source_id || "").localeCompare(String(right.source_id || "")),
  );
  return (
    <div className="h-full min-h-0 overflow-auto p-4 lg:p-6">
      <div className="grid gap-2">
        {rows.map((source) => {
          const sourceId = String(source.source_id || "");
          const canDelete = capabilityCenterSourceRemovalAction(source) !== "none";
          return (
            <div key={sourceId} className={capabilityCenterSourcesGridClass()}>
              <div className="min-w-0">
                <div className="break-words font-medium [overflow-wrap:anywhere]">{sourceId}</div>
                <div className="mt-0.5 text-xs text-[var(--color-text-secondary)]">
                  {t("capabilityCenter.sources.records", { count: source.record_count || 0 })}
                </div>
              </div>
              {canDelete ? (
                <TooltipIconButton label={t("capabilityCenter.sources.delete")}>
                  {(referenceProps, setReference) => (
                    <span ref={setReference} {...referenceProps}>
                      <button
                        type="button"
                        className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-rose-500/30 bg-rose-500/10 text-xs text-rose-600 hover:bg-rose-500/15 disabled:opacity-60 dark:text-rose-300 md:h-8 md:w-8"
                        disabled={props.busyKey === `source-delete:${sourceId}`}
                        aria-label={
                          props.busyKey === `source-delete:${sourceId}`
                            ? t("capabilityCenter.sources.deleting")
                            : t("capabilityCenter.sources.delete")
                        }
                        onClick={() => props.onDelete(source)}
                        data-status-tone="error"
                      >
                        <LcdIcon icon={Trash2} glyph="trash" size={14} aria-hidden="true" />
                      </button>
                    </span>
                  )}
                </TooltipIconButton>
              ) : null}
            </div>
          );
        })}
        {rows.length === 0 ? (
          <div className="text-sm text-[var(--color-text-muted)]">
            {t("capabilityCenter.sources.empty")}
          </div>
        ) : null}
      </div>
      <CapabilitySourceInstances
        instances={props.sourceInstances}
        busyKey={props.busyKey}
        onDelete={props.onDeleteInstance}
      />
    </div>
  );
}

export function SourcesSummary({ sources }: { sources: Record<string, CapabilitySourceState> }) {
  const { t } = useTranslation("settings");
  return (
    <aside className="hidden min-h-0 min-w-0 overflow-x-hidden overflow-y-auto bg-[var(--glass-panel-bg)] p-5 lg:block">
      <div className="text-xs uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
        {t("capabilityCenter.sections.sources.label")}
      </div>
      <h3 className="mt-1 text-lg font-semibold">{t("capabilityCenter.sections.sources.hint")}</h3>
      <div className="mt-5 grid gap-2">
        <SourceStat
          label={t("capabilityCenter.sources.total")}
          value={Object.keys(sources).length}
        />
      </div>
    </aside>
  );
}
