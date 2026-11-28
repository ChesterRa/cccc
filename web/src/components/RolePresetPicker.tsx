import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BUILTIN_ROLE_PRESETS,
  getRolePresetApplyState,
  getRolePresetById,
} from "../utils/rolePresets";
import { GroupCombobox } from "./GroupCombobox";

type RolePresetPickerProps = {
  draftValue: string;
  disabled?: boolean;
  /** The label of the field the chosen preset fills in; it shares one row with the picker. */
  label: ReactNode;
  onChangeDraft: (value: string) => void;
};

const NO_PRESET_VALUE = "__no_actor_preset__";

export function RolePresetPicker({
  draftValue,
  disabled = false,
  label,
  onChangeDraft,
}: RolePresetPickerProps) {
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

  const normalizedDraftValue = String(draftValue || "").trim();
  const selectedPreset = getRolePresetById(selectedPresetId);
  const matchingPresetId =
    selectedPreset?.content.trim() === normalizedDraftValue ? selectedPresetId : "";
  const visibleNotice =
    notice && notice.presetId === matchingPresetId && notice.draftValue === normalizedDraftValue
      ? t(notice.kind === "applied" ? "rolePresetAppliedNotice" : "rolePresetAlreadyMatchesDraft", {
          name: localizedPreset(notice.presetId)?.name || "",
        })
      : "";

  const choosePreset = (presetId: string) => {
    const preset = localizedPreset(presetId);
    if (!preset || disabled) return;
    const applyState = getRolePresetApplyState(draftValue, preset.content);
    const nextDraftValue = String(preset.content || "").trim();
    if (applyState === "no_change") {
      setSelectedPresetId(preset.id);
      setNotice({ presetId: preset.id, kind: "no_change", draftValue: nextDraftValue });
      return;
    }
    // Declining keeps both the draft and the previous choice: nothing was filled in.
    if (
      applyState === "confirm_replace" &&
      !window.confirm(t("rolePresetReplaceConfirm", { name: preset.name }))
    ) {
      return;
    }
    setSelectedPresetId(preset.id);
    onChangeDraft(preset.content);
    setNotice({ presetId: preset.id, kind: "applied", draftValue: nextDraftValue });
  };

  return (
    <div className="space-y-2">
      <div className="flex min-w-0 flex-wrap items-end justify-between gap-x-3 gap-y-2">
        <div className="min-w-0">{label}</div>
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
          value={matchingPresetId}
          onChange={(nextValue) => {
            if (nextValue === NO_PRESET_VALUE) {
              setSelectedPresetId("");
              setNotice(null);
              return;
            }
            choosePreset(nextValue);
          }}
          placeholder={t("rolePresetFill")}
          searchPlaceholder={t("rolePresetFill")}
          emptyText={t("common:noResults", { defaultValue: "No matching results" })}
          ariaLabel={t("rolePreset")}
          triggerClassName="glass-input min-h-[36px] w-full px-3 py-1.5 text-xs sm:w-56"
          contentClassName="p-0"
          disabled={disabled}
          searchable={false}
        />
      </div>

      {visibleNotice ? (
        <div role="status" className="text-xs text-[var(--color-text-secondary)]">
          {visibleNotice}
        </div>
      ) : null}
    </div>
  );
}
