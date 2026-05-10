import { Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { CapabilitySourceInstance } from "../../types";

type CapabilitySourceInstancesProps = {
  instances: CapabilitySourceInstance[];
  busyKey: string;
  onDelete: (instance: CapabilitySourceInstance) => void;
};

export function CapabilitySourceInstances({ instances, busyKey, onDelete }: CapabilitySourceInstancesProps) {
  const { t } = useTranslation("settings");
  if (!instances.length) return null;

  return (
    <div className="mt-5">
      <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
        {t("capabilityCenter.sources.instances")}
      </div>
      <div className="grid gap-2">
        {instances.map((instance) => {
          const key = String(instance.source_instance_key || "");
          return (
            <div
              key={key}
              className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2 text-sm"
            >
              <div className="min-w-0">
                <div className="break-words font-medium [overflow-wrap:anywhere]">{instance.label || key}</div>
                <div className="mt-0.5 line-clamp-2 text-xs text-[var(--color-text-muted)]">
                  {t("capabilityCenter.sources.instanceMeta", {
                    source: instance.source_id || "-",
                    count: Number(instance.record_count || 0),
                  })}
                  {instance.source_uri ? ` · ${instance.source_uri}` : ""}
                </div>
              </div>
              <button
                type="button"
                className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-rose-500/30 bg-rose-500/10 text-xs text-rose-600 hover:bg-rose-500/15 disabled:opacity-60 dark:text-rose-300"
                disabled={busyKey === `source-instance-delete:${key}`}
                aria-label={t("capabilityCenter.sources.deleteInstance")}
                onClick={() => onDelete(instance)}
              >
                <Trash2 size={14} aria-hidden="true" />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
