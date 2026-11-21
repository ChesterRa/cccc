import type React from "react";
import { Eye, EyeOff, Power, PowerOff, Shield, Trash2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { LcdIcon } from "../Icons";
import type { LcdGlyphName } from "../LcdGlyph";
import { useTranslation } from "react-i18next";

import type { CapabilityOverviewItem } from "../../types";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import {
  capabilityCenterDisplayName,
  type CapabilityCenterRemovalAction,
} from "./capabilityCenterModel";

interface CapabilityControlsPanelProps {
  row: CapabilityOverviewItem;
  enabled: boolean;
  hidden: boolean;
  blocked: boolean;
  /** False hides enabling when no member of the Group could use it; disabling stays. */
  canEnable?: boolean;
  /** Where the block to lift was set: a Group block is lifted in that Group only. */
  blockScope?: "global" | "group";
  canShowSlashToggle: boolean;
  removalAction: CapabilityCenterRemovalAction;
  busyKey: string;
  groupId: string;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onRemove: (
    row: CapabilityOverviewItem,
    action: Exclude<CapabilityCenterRemovalAction, "none">,
  ) => void;
}

/** The actions under a capability's name; each label says where it applies. */
export function CapabilityControlsPanel({
  row,
  enabled,
  hidden,
  blocked,
  canEnable = true,
  blockScope = "global",
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
  const disabled = !String(groupId || "").trim();
  const canRemove = removalAction !== "none";

  return (
    <section className="mt-3 min-w-0" aria-label={t("capabilityCenter.controls.title")}>
      <div className="flex flex-wrap gap-2">
        {enabled || canEnable ? (
          <ControlButton
            icon={enabled ? PowerOff : Power}
            glyph={enabled ? "power-off" : "power"}
            label={
              enabled
                ? t("capabilityCenter.controls.disableForGroup")
                : t("capabilityCenter.controls.enableForGroup")
            }
            meta={
              enabled
                ? t("capabilityCenter.enable.disableTitle")
                : t("capabilityCenter.enable.enableTitle")
            }
            tone={enabled ? "neutral" : "primary"}
            busy={busyKey === `enable:${capId}` || disabled}
            onClick={() => onToggleEnable(row)}
          />
        ) : null}
        {canShowSlashToggle ? (
          <ControlButton
            icon={hidden ? Eye : EyeOff}
            glyph={hidden ? "eye" : "eye-off"}
            label={
              hidden
                ? t("capabilityCenter.showInSlashCommands")
                : t("capabilityCenter.hideFromSlashCommands")
            }
            meta={
              hidden
                ? t("capabilityCenter.controls.slashHiddenMeta")
                : t("capabilityCenter.controls.slashVisibleMeta")
            }
            busy={busyKey === `slash:${capId}` || disabled}
            onClick={() => onToggleSlash(row)}
          />
        ) : null}
        <ControlButton
          icon={Shield}
          glyph="shield"
          label={
            blocked
              ? t("capabilityCenter.block.unblock")
              : t("capabilityCenter.controls.blockEverywhere")
          }
          meta={
            blocked
              ? blockScope === "group"
                ? t("capabilityCenter.controls.unblockGroupTitle")
                : t("capabilityCenter.block.unblockTitle")
              : t("capabilityCenter.block.blockTitle")
          }
          tone={blocked ? "neutral" : "warn"}
          busy={busyKey === `block:${capId}` || disabled}
          onClick={() => onToggleBlock(row)}
        />
        {canRemove ? (
          <ControlButton
            icon={Trash2}
            glyph="trash"
            label={t(`capabilityCenter.remove.label.${removalAction}`)}
            meta={t(`capabilityCenter.remove.title.${removalAction}`, {
              name: capabilityCenterDisplayName(row),
            })}
            tone="danger"
            busy={busyKey === `remove:${capId}` || disabled}
            onClick={() => onRemove(row, removalAction)}
          />
        ) : null}
      </div>
      {disabled ? (
        <p className="mt-2 text-xs text-[var(--color-text-muted)]">
          {t("capabilityCenter.controls.openGroup")}
        </p>
      ) : null}
    </section>
  );
}

function ControlButton({
  icon: Icon,
  glyph,
  label,
  meta,
  tone = "neutral",
  busy,
  onClick,
}: {
  icon: LucideIcon;
  glyph: LcdGlyphName;
  label: string;
  meta: string;
  tone?: "primary" | "neutral" | "warn" | "danger";
  busy: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <Button
      type="button"
      size="sm"
      variant={tone === "primary" ? "default" : tone === "danger" ? "destructive" : "outline"}
      className={cn(
        "text-sm",
        tone === "neutral" ? "text-[var(--color-text-primary)]" : "",
        tone === "warn"
          ? "border-amber-500/40 text-amber-700 hover:bg-amber-500/10 dark:text-amber-300"
          : "",
      )}
      disabled={busy}
      title={meta}
      onClick={onClick}
      data-status-tone={tone === "danger" ? "error" : tone === "warn" ? "warning" : undefined}
      data-lcd-key={tone === "neutral" ? "" : undefined}
    >
      <LcdIcon icon={Icon} glyph={glyph} size={14} aria-hidden="true" />
      <span>{label}</span>
    </Button>
  );
}
