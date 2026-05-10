import type React from "react";
import { Eye, EyeOff, Power, PowerOff, Shield, Trash2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { CapabilityOverviewItem } from "../../types";
import { cn } from "../../lib/utils";
import {
  capabilityCenterDisplayName,
  capabilityCenterType,
  type CapabilityCenterRemovalAction,
} from "./capabilityCenterModel";

interface CapabilityControlsPanelProps {
  row: CapabilityOverviewItem;
  enabled: boolean;
  hidden: boolean;
  blocked: boolean;
  canShowSlashToggle: boolean;
  removalAction: CapabilityCenterRemovalAction;
  busyKey: string;
  groupId: string;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onRemove: (row: CapabilityOverviewItem, action: Exclude<CapabilityCenterRemovalAction, "none">) => void;
}

export function CapabilityControlsPanel({
  row,
  enabled,
  hidden,
  blocked,
  canShowSlashToggle,
  removalAction,
  busyKey,
  groupId,
  onToggleEnable,
  onToggleSlash,
  onToggleBlock,
  onRemove,
}: CapabilityControlsPanelProps) {
  const { t } = useTranslation("settings");
  const capId = String(row.capability_id || "").trim();
  const type = capabilityCenterType(row);
  const disabled = !String(groupId || "").trim();
  const canRemove = removalAction !== "none";

  return (
    <section className="mt-5 min-w-0 border-t border-[var(--glass-border-subtle)] pt-4">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
        {t("capabilityCenter.controls.title")}
      </div>
      <div className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
        {type === "mcp" ? t("capabilityCenter.controls.mcpHint") : t("capabilityCenter.controls.skillHint")}
      </div>
      <div className="mt-3 grid gap-2">
        <ControlButton
          icon={enabled ? PowerOff : Power}
          label={enabled ? t("capabilityCenter.enable.disable") : t("capabilityCenter.enable.enable")}
          meta={enabled ? t("capabilityCenter.controls.enabledMeta") : t("capabilityCenter.controls.disabledMeta")}
          busy={busyKey === `enable:${capId}` || disabled}
          onClick={() => onToggleEnable(row)}
        />
        {canShowSlashToggle ? (
          <ControlButton
            icon={hidden ? Eye : EyeOff}
            label={hidden ? t("capabilityCenter.showInSlashCommands") : t("capabilityCenter.hideFromSlashCommands")}
            meta={hidden ? t("capabilityCenter.controls.slashHiddenMeta") : t("capabilityCenter.controls.slashVisibleMeta")}
            busy={busyKey === `slash:${capId}` || disabled}
            onClick={() => onToggleSlash(row)}
          />
        ) : null}
        <ControlButton
          icon={Shield}
          label={blocked ? t("capabilityCenter.block.unblock") : t("capabilityCenter.block.block")}
          meta={blocked ? t("capabilityCenter.controls.blockedMeta") : t("capabilityCenter.controls.blockMeta")}
          tone={blocked ? "neutral" : "warn"}
          busy={busyKey === `block:${capId}` || disabled}
          onClick={() => onToggleBlock(row)}
        />
        {canRemove ? (
          <ControlButton
            icon={Trash2}
            label={t(`capabilityCenter.remove.label.${removalAction}`)}
            meta={t(`capabilityCenter.remove.title.${removalAction}`, { name: capabilityCenterDisplayName(row) })}
            tone="danger"
            busy={busyKey === `remove:${capId}` || disabled}
            onClick={() => onRemove(row, removalAction)}
          />
        ) : null}
      </div>
      {disabled ? (
        <p className="mt-2 text-xs text-[var(--color-text-muted)]">{t("capabilityCenter.controls.openGroup")}</p>
      ) : null}
    </section>
  );
}

function ControlButton({
  icon: Icon,
  label,
  meta,
  tone = "neutral",
  busy,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  meta: string;
  tone?: "neutral" | "warn" | "danger";
  busy: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "grid min-h-[44px] grid-cols-[32px_minmax(0,1fr)] items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60",
        tone === "danger"
          ? "border-rose-500/25 bg-rose-500/10 text-rose-700 hover:bg-rose-500/15 dark:text-rose-300"
          : tone === "warn"
            ? "border-amber-500/30 bg-amber-500/10 text-amber-700 hover:bg-amber-500/15 dark:text-amber-300"
            : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-bg-hover)]",
      )}
      disabled={busy}
      title={meta}
      onClick={onClick}
    >
      <span className="flex h-8 w-8 items-center justify-center rounded-md bg-black/5 dark:bg-white/8">
        <Icon size={15} aria-hidden="true" />
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-medium">{label}</span>
        <span className="block truncate text-[11px] text-[var(--color-text-muted)]">{meta}</span>
      </span>
    </button>
  );
}
