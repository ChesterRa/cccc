import { useState } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../utils/classNames";
import { BUILTIN_ROLE_PRESETS, getRolePresetApplyState, getRolePresetById } from "../utils/rolePresets";
import { Button } from "./ui/button";
import { GroupCombobox } from "./GroupCombobox";

type RolePresetPickerProps = {
  draftValue: string;
  disabled?: boolean;
  onChangeDraft: (value: string) => void;
};

const NO_PRESET_VALUE = "__no_actor_preset__";

export function RolePresetPicker({ draftValue, disabled = false, onChangeDraft }: RolePresetPickerProps) {
  const { t } = useTranslation("actors");
  const [selectedPresetId, setSelectedPresetId] = useState("");
  const [notice, setNotice] = useState<{
    presetId: string;
    kind: "applied" | "no_change";
    draftValue: string;
  } | null>(null);

  const localizedPreset = (presetId: string) => {
    const preset = getRolePresetById(presetId);
    if (!preset) return null;
    return {
      ...preset,
      name: t(`rolePresetCatalog.${preset.id}.name`, { defaultValue: preset.name }),
      summary: t(`rolePresetCatalog.${preset.id}.summary`, { defaultValue: preset.summary }),
      useWhen: t(`rolePresetCatalog.${preset.id}.useWhen`, { defaultValue: preset.useWhen }),
    };
  };

  const selectedPreset = localizedPreset(selectedPresetId);
  const normalizedDraftValue = String(draftValue || "").trim();
  const visibleNotice =
    notice && notice.presetId === selectedPresetId && notice.draftValue === normalizedDraftValue
      ? t(
          notice.kind === "applied" ? "rolePresetAppliedNotice" : "rolePresetAlreadyMatchesDraft",
          { name: localizedPreset(notice.presetId)?.name || "" }
        )
      : "";

  const applyPreset = () => {
    if (!selectedPreset || disabled) return;
    const applyState = getRolePresetApplyState(draftValue, selectedPreset.content);
    const nextDraftValue = String(selectedPreset.content || "").trim();
    if (applyState === "no_change") {
      setNotice({ presetId: selectedPreset.id, kind: "no_change", draftValue: nextDraftValue });
      return;
    }
    if (
      applyState === "confirm_replace" &&
      !window.confirm(t("rolePresetReplaceConfirm", { name: selectedPreset.name }))
    ) {
      return;
    }
    onChangeDraft(selectedPreset.content);
    setNotice({ presetId: selectedPreset.id, kind: "applied", draftValue: nextDraftValue });
  };

  return (
    <div className="space-y-2">
      <div className="min-w-0">
        <div className="text-xs font-semibold text-[var(--color-text-primary)]">{t("rolePreset")}</div>
        <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{t("rolePresetHint")}</div>
      </div>

      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <GroupCombobox
          items={[
            {
              value: NO_PRESET_VALUE,
              label: t("rolePresetNone"),
              description: t("rolePresetNoneDescription"),
              keywords: [t("rolePresetNone"), t("rolePresetNoneDescription")],
            },
            ...BUILTIN_ROLE_PRESETS.map((preset) => {
              const localized = localizedPreset(preset.id) || preset;
              return {
                value: preset.id,
                label: localized.name,
                description: localized.summary,
                keywords: [localized.name, localized.summary, localized.useWhen],
              };
            }),
          ]}
          value={selectedPresetId}
          onChange={(nextValue) => {
            if (nextValue === NO_PRESET_VALUE) {
              setSelectedPresetId("");
              setNotice(null);
              return;
            }
            setSelectedPresetId(nextValue);
            setNotice(null);
          }}
          placeholder={t("rolePresetSelectPlaceholder")}
          searchPlaceholder={t("rolePresetSelectPlaceholder")}
          emptyText={t("common:noResults", { defaultValue: "No matching results" })}
          ariaLabel={t("rolePreset")}
          triggerClassName="glass-input w-full flex-1 min-h-[44px] px-3 py-2.5 text-sm"
          contentClassName="p-0"
          disabled={disabled}
          searchable={false}
          matchTriggerWidth
        />

        <Button
          type="button"
          className={classNames("sm:shrink-0", !selectedPreset && "text-[var(--color-text-muted)]")}
          variant={selectedPreset && !disabled ? "default" : "secondary"}
          onClick={applyPreset}
          disabled={!selectedPreset || disabled}
        >
          {t("applyPreset")}
        </Button>
      </div>

      <div className="text-[10px] leading-5 text-[var(--color-text-muted)]">
        {selectedPreset ? selectedPreset.summary : t("rolePresetSelectPrompt")}
      </div>

      {visibleNotice ? (
        <div className="rounded-xl border border-black/10 bg-[rgb(245,245,245)] px-3 py-2 text-xs text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
          {visibleNotice}
        </div>
      ) : null}
    </div>
  );
}
