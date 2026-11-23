import { useTranslation } from "react-i18next";
import type { RuntimeCategory } from "./runtimeCategory";

export function RuntimeCategoryPicker({
  value,
  current,
  canSwitch,
  choices = ["managed", "native", "web"],
  disabled,
  onChange,
}: {
  value: RuntimeCategory;
  current: RuntimeCategory;
  canSwitch: boolean;
  choices?: readonly RuntimeCategory[];
  disabled?: boolean;
  onChange: (value: RuntimeCategory) => void;
}) {
  const { t } = useTranslation("actors");
  const unavailable = (choice: RuntimeCategory) =>
    !canSwitch && choice !== current && (choice === "native" || current === "native");
  return (
    <div>
      <div className="mb-2 text-xs font-medium text-[var(--color-text-muted)]">
        {t("runtimeSwitch.category")}
      </div>
      <div
        className={`grid gap-1.5 ${choices.length === 2 ? "grid-cols-2" : "grid-cols-3"}`}
        role="group"
        aria-label={t("runtimeSwitch.category")}
      >
        {choices.map((choice) => (
          <button
            key={choice}
            type="button"
            disabled={disabled || unavailable(choice)}
            aria-pressed={value === choice}
            onClick={() => onChange(choice)}
            className={[
              "min-h-11 min-w-0 rounded-xl border px-1.5 py-2 text-xs font-medium transition-colors",
              "disabled:cursor-not-allowed disabled:opacity-45",
              value === choice
                ? "border-[var(--color-text-primary)] bg-[var(--color-text-primary)] text-[var(--color-bg-primary)]"
                : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
            ].join(" ")}
          >
            {t(`runtimeSwitch.${choice}`)}
          </button>
        ))}
      </div>
      {!canSwitch ? (
        <p className="mt-2 text-xs text-[var(--color-text-muted)]">{t("runtimeSwitch.notAdmin")}</p>
      ) : null}
    </div>
  );
}
