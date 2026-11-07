import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";

export function DeepSeekPendingChanges({
  disabled,
  onSave,
  onSaveAndRestart,
}: {
  disabled: boolean;
  onSave?: () => void;
  onSaveAndRestart?: () => void;
}) {
  const { t } = useTranslation("actors");
  return (
    <div
      role="status"
      className="flex flex-wrap items-center justify-between gap-3 border border-[var(--glass-border-subtle)] p-3 text-xs text-[var(--color-text-secondary)]"
    >
      <p className="font-semibold text-[var(--color-text-primary)]">
        {t(onSaveAndRestart ? "deepseek.pendingTitle" : "deepseek.createPendingHint")}
      </p>
      {onSaveAndRestart ? (
        <div className="flex flex-wrap gap-2">
          {onSave ? (
            <Button type="button" variant="outline" size="sm" disabled={disabled} onClick={onSave}>
              {t("common:save")}
            </Button>
          ) : null}
          <Button type="button" size="sm" disabled={disabled} onClick={onSaveAndRestart}>
            {t("saveAndRestart")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
