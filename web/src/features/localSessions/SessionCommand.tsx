import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckIcon, CopyIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { copyTextToClipboard } from "../../utils/copy";

export function SessionCommand({
  command,
  copyLabel,
  disabled = false,
  emphasis,
}: {
  command: string;
  copyLabel?: string;
  disabled?: boolean;
  emphasis?: "primary" | "secondary";
}) {
  const { t } = useTranslation("layout");
  const [result, setResult] = useState<"idle" | "copied" | "failed">("idle");
  const label =
    result === "copied" ? t("common:copied") : copyLabel || t("localSessions.copyCommand");
  return (
    <div className="space-y-1">
      <div className="flex min-w-0 items-center gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-2">
        <pre
          dir="ltr"
          className={
            emphasis
              ? "min-w-0 flex-1 whitespace-pre-wrap px-1 py-1 font-mono text-xs leading-relaxed sm:text-sm"
              : "min-w-0 flex-1 overflow-x-auto px-1 py-2 font-mono text-sm"
          }
        >
          {emphasis
            ? command.split(/( +)/).map((part, index) =>
                part.trim() ? (
                  <span key={index} className="whitespace-nowrap">
                    {part}
                  </span>
                ) : (
                  part
                ),
              )
            : command}
        </pre>
        <Button
          type="button"
          variant={emphasis === "primary" ? "default" : "ghost"}
          size={emphasis ? "sm" : "icon"}
          className={
            emphasis
              ? "min-h-10 max-sm:min-h-11 shrink-0 gap-1.5 px-2"
              : "h-[44px] w-[44px] shrink-0"
          }
          title={label}
          aria-label={label}
          disabled={disabled}
          onClick={async () =>
            setResult((await copyTextToClipboard(command)) ? "copied" : "failed")
          }
        >
          {result === "copied" ? (
            <CheckIcon size={16} aria-hidden="true" />
          ) : (
            <CopyIcon size={16} aria-hidden="true" />
          )}
          {emphasis &&
            (result === "copied" ? t("common:copied") : t("sessionNodes.copyCommandText"))}
        </Button>
      </div>
      {result !== "idle" ? (
        <p role={result === "failed" ? "alert" : "status"} className="text-sm">
          {t(result === "copied" ? "common:copied" : "common:copyFailed")}
        </p>
      ) : null}
    </div>
  );
}
