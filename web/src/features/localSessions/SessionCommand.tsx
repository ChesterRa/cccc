import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckIcon, CopyIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { copyTextToClipboard } from "../../utils/copy";

export function SessionCommand({ command }: { command: string }) {
  const { t } = useTranslation("layout");
  const [result, setResult] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <div className="space-y-1">
      <div className="flex min-w-0 items-center gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-2">
        <pre className="min-w-0 flex-1 overflow-x-auto px-1 py-2 font-mono text-sm">{command}</pre>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-[44px] w-[44px] shrink-0"
          title={t(result === "copied" ? "common:copied" : "localSessions.copyCommand")}
          aria-label={t(result === "copied" ? "common:copied" : "localSessions.copyCommand")}
          onClick={async () =>
            setResult((await copyTextToClipboard(command)) ? "copied" : "failed")
          }
        >
          {result === "copied" ? (
            <CheckIcon size={16} aria-hidden="true" />
          ) : (
            <CopyIcon size={16} aria-hidden="true" />
          )}
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
