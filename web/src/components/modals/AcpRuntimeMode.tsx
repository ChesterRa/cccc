import { useTranslation } from "react-i18next";
import { RUNTIME_INFO, type RuntimeMode } from "../../types";
import { SelectCombobox } from "../SelectCombobox";
import { useGroupStore } from "../../stores";
import { runtimeDetectionKey } from "../../utils/runtimeDetection";

export function AcpRuntimeMode({
  runtime,
  value,
  onChange,
  disabled,
}: {
  runtime: string;
  value: RuntimeMode;
  onChange(value: RuntimeMode): void;
  disabled?: boolean;
}) {
  const { t } = useTranslation("actors");
  const runtimeName = runtime;
  const info = useGroupStore((s) => s.runtimes.find((runtime) => runtime.name === runtimeName));
  const status = useGroupStore((s) => s.runtimeDetectionStatus);
  return (
    <div className="mt-3 space-y-2">
      <label className="block text-xs font-medium text-[var(--color-text-muted)]">
        {t("acpMode.label", { runtime: RUNTIME_INFO[runtime]?.label || runtime })}
      </label>
      <SelectCombobox
        className="w-full min-h-[44px] rounded-xl border px-4 py-2.5 text-sm glass-input text-[var(--color-text-primary)] [&>span]:whitespace-normal"
        contentClassName="[&_[role=option]_.truncate]:whitespace-normal"
        value={value}
        onChange={(mode) => onChange(mode as RuntimeMode)}
        disabled={disabled}
        ariaLabel={t("acpMode.label", { runtime: RUNTIME_INFO[runtime]?.label || runtime })}
        items={[
          {
            value: "default",
            label: t("antigravityMode.tui"),
            description: t(`runtimeDetection.${runtimeDetectionKey(runtime, info, status)}`),
          },
          {
            value: "acp",
            label: t("antigravityMode.acp"),
            description: t(`runtimeDetection.${runtimeDetectionKey(runtime, info, status, "acp")}`),
          },
        ]}
      />
      {value === "acp" && (
        <p className="text-xs leading-5 text-[var(--color-text-muted)]">
          {t(runtime === "antigravity" ? "antigravityMode.hint" : "acpMode.hint")}
          {runtime === "antigravity" && (
            <>
              <br />
              <code className="break-all">
                cccc setup --runtime antigravity --runtime-mode acp --login
              </code>
            </>
          )}
        </p>
      )}
    </div>
  );
}
