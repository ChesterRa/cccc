import { type ReactNode, useId } from "react";
import { useTranslation } from "react-i18next";

import { RolePresetPicker } from "../RolePresetPicker";
import { Textarea } from "../ui/textarea";

interface ActorNotesFieldProps {
  value: string;
  onChange: (value: string) => void;
  /** Blocks typing, for example while the saved notes are still loading. */
  disabled?: boolean;
  /** Blocks filling in a preset; defaults to `disabled`. */
  presetDisabled?: boolean;
  /** Loading or error state shown under the text area. */
  children?: ReactNode;
}

/** The Agent's notes, with built-in presets one choice away in the label row. */
export function ActorNotesField({
  value,
  onChange,
  disabled = false,
  presetDisabled = disabled,
  children,
}: ActorNotesFieldProps) {
  const { t } = useTranslation("actors");
  const fieldId = useId();
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-2">
      <RolePresetPicker
        label={
          <label
            htmlFor={fieldId}
            className="block text-xs font-medium text-[var(--color-text-muted)]"
          >
            {t("actorNotes")}
          </label>
        }
        draftValue={value}
        onChangeDraft={onChange}
        disabled={presetDisabled}
      />
      <Textarea
        id={fieldId}
        className="min-h-[120px] flex-auto"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={t("actorNotesPlaceholder")}
        spellCheck={false}
        disabled={disabled}
      />
      {children}
    </div>
  );
}
