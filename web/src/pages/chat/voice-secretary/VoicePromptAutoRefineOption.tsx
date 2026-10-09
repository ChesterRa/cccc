import { useTranslation } from "react-i18next";

export function VoicePromptAutoRefineOption({
  checked,
  disabled,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useTranslation("chat");
  return (
    <label className="flex min-h-11 cursor-pointer items-center gap-2 text-xs text-[var(--color-text-secondary)] has-disabled:cursor-not-allowed has-disabled:opacity-50 sm:min-h-7 pointer-coarse:min-h-11">
      <input
        type="checkbox"
        className="m-0 h-4 w-4 shrink-0 cursor-[inherit] accent-[var(--primary)]"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>{t("voiceSecretaryPromptAutoRefine")}</span>
    </label>
  );
}
