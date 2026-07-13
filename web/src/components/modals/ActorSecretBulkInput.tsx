import { useId, useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { parsePrivateEnvSetText } from "../../utils/privateEnvInput";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";

interface ActorSecretBulkInputProps {
  disabled: boolean;
  onCancel: () => void;
  onApply: (setVars: Record<string, string>) => void;
}

export function ActorSecretBulkInput({ disabled, onCancel, onApply }: ActorSecretBulkInputProps) {
  const { t } = useTranslation("actors");
  const errorId = useId();
  const [text, setText] = useState("");
  const [error, setError] = useState("");

  const close = () => {
    setText("");
    setError("");
    onCancel();
  };

  const apply = () => {
    const parsed = parsePrivateEnvSetText(text);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    onApply(parsed.setVars);
  };

  return (
    <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[11px] font-semibold text-[var(--color-text-primary)]">
            {t("secretManager.batchPaste")}
          </div>
          <div className="mt-1 text-[10px] leading-relaxed text-[var(--color-text-muted)]">
            {t("secretManager.batchHint")}
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="w-9 shrink-0 px-0"
          disabled={disabled}
          aria-label={t("common:cancel")}
          title={t("common:cancel")}
          onClick={close}
        >
          <X size={15} aria-hidden="true" />
        </Button>
      </div>
      <Textarea
        aria-label={t("secretManager.batchPaste")}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? errorId : undefined}
        value={text}
        disabled={disabled}
        className="mt-2 min-h-[96px] font-mono text-xs"
        placeholder={t("secretManager.batchPlaceholder")}
        spellCheck={false}
        onChange={(event) => {
          setText(event.target.value);
          setError("");
        }}
      />
      {error ? (
        <div
          id={errorId}
          role="alert"
          className="mt-2 text-[10px] text-rose-600 dark:text-rose-400"
        >
          {error}
        </div>
      ) : null}
      <div className="mt-2 flex justify-end">
        <Button type="button" size="sm" disabled={disabled || !text.trim()} onClick={apply}>
          {t("secretManager.batchApply")}
        </Button>
      </div>
    </div>
  );
}
