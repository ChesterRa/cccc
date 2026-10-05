import { supportsAcpMode } from "../../types";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { useGroupStore } from "../../stores";
import { RUNTIME_INFO, SUPPORTED_RUNTIMES, type RuntimeMode } from "../../types";
import { runtimeDetectionKey } from "../../utils/runtimeDetection";
import { SelectCombobox } from "../SelectCombobox";
import { Button } from "../ui/button";

export function RuntimeSelector({
  value,
  onChange,
  runtimeMode = "default",
  disabled = false,
  allowUndetected = false,
  ariaLabel,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  runtimeMode?: RuntimeMode;
  disabled?: boolean;
  allowUndetected?: boolean;
  ariaLabel?: string;
  className?: string;
}) {
  const { t } = useTranslation("actors");
  const runtimes = useGroupStore((s) => s.runtimes);
  const status = useGroupStore((s) => s.runtimeDetectionStatus);
  const refresh = useGroupStore((s) => s.refreshRuntimes);
  useEffect(() => {
    if (status === "idle") void refresh();
  }, [status, refresh]);
  const selectedInfo = runtimes.find((item) => item.name === value);
  const selectedKey = runtimeDetectionKey(value, selectedInfo, status, runtimeMode);
  const captionKey = status === "error" ? "failed" : selectedKey;
  const checking = status === "idle" || status === "loading";
  const modeLabel = supportsAcpMode(value) ? (runtimeMode === "acp" ? "ACP" : "TUI") : "";

  return (
    <div className="space-y-1">
      <SelectCombobox
        className={
          className ||
          "w-full min-h-[44px] rounded-xl border px-4 py-2.5 text-sm glass-input text-[var(--color-text-primary)]"
        }
        contentClassName="[&_[role=option]_.truncate]:whitespace-normal"
        value={value}
        onChange={onChange}
        disabled={disabled}
        ariaLabel={ariaLabel || t("runtime")}
        items={SUPPORTED_RUNTIMES.map((runtime) => {
          const info = runtimes.find((item) => item.name === runtime);
          const key = runtimeDetectionKey(runtime, info, status);
          const description =
            runtime === "antigravity"
              ? `TUI: ${t(`runtimeDetection.${key}`)} · ACP: ${t(`runtimeDetection.${runtimeDetectionKey(runtime, info, status, "acp")}`)}`
              : t(`runtimeDetection.${key}`);
          return {
            value: runtime,
            label: RUNTIME_INFO[runtime]?.label || runtime,
            description,
            disabled: !allowUndetected && runtime !== "antigravity" && key === "missing",
          };
        })}
        searchable
      />
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 text-xs">
        <span
          role="status"
          className={
            captionKey === "failed" || captionKey === "missing"
              ? "text-orange-700 dark:text-orange-300"
              : "text-[var(--color-text-muted)]"
          }
        >
          {modeLabel ? `${modeLabel}: ` : ""}
          {t(`runtimeDetection.${captionKey}`)}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="min-h-[44px] gap-1.5 text-xs"
          disabled={disabled || checking}
          onClick={() => void refresh()}
        >
          <RefreshCw
            size={13}
            aria-hidden="true"
            className={checking ? "animate-spin motion-reduce:animate-none" : ""}
          />
          {t("runtimeDetection.recheck")}
        </Button>
      </div>
      <p className="text-[11px] leading-4 text-[var(--color-text-muted)]">
        {t("runtimeDetection.scope")}
      </p>
    </div>
  );
}
