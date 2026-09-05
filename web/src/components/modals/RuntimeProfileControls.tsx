import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { RUNTIME_INFO, type ActorProfile } from "../../types";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { SelectCombobox } from "../SelectCombobox";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";
import { formatRuntimeCommand, runtimeProfileScopeLabel } from "./runtimeProfileControlsModel";

export type RuntimeConfigurationMode = "custom" | "profile";

function modeButtonClass(selected: boolean): string {
  return [
    "px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-all ease-spring duration-300",
    selected
      ? "border-[var(--color-text-primary)] bg-[var(--color-text-primary)] text-[var(--color-bg-primary)] hover:bg-[var(--color-text-primary)] hover:text-[var(--color-bg-primary)] hover:opacity-90"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
  ].join(" ");
}

export function OpenCodeManagedModelHint({ runtime }: { runtime?: string | null }) {
  const { t } = useTranslation("actors");
  if (
    !["opencode", "kilo"].includes(
      String(runtime || "")
        .trim()
        .toLowerCase(),
    )
  )
    return null;
  return (
    <p className="mt-1.5 flex items-start gap-1.5 text-xs font-medium leading-5 text-orange-700 dark:text-orange-300">
      <AlertTriangle size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0">{t("opencodeManagedModelHint")}</span>
    </p>
  );
}

export function RuntimeConfigurationModePicker({
  value,
  disabled,
  onChange,
}: {
  value: RuntimeConfigurationMode;
  disabled?: boolean;
  onChange: (value: RuntimeConfigurationMode) => void;
}) {
  const { t } = useTranslation("actors");
  return (
    <div>
      <div className="mb-2 text-xs font-medium text-[var(--color-text-muted)]">
        {t("creationMode")}
      </div>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <Button
          type="button"
          variant="outline"
          className={modeButtonClass(value === "custom")}
          onClick={() => onChange("custom")}
          disabled={disabled}
        >
          {t("customAgent")}
        </Button>
        <Button
          type="button"
          variant="outline"
          className={modeButtonClass(value === "profile")}
          onClick={() => onChange("profile")}
          disabled={disabled}
        >
          {t("fromActorProfile")}
        </Button>
      </div>
    </div>
  );
}

export function RuntimeProfilePicker({
  value,
  profiles,
  busy,
  disabled,
  emptyHint,
  hostNote,
  detailsLabel,
  onChange,
}: {
  value: string;
  profiles: ActorProfile[];
  busy: boolean;
  disabled?: boolean;
  emptyHint?: string;
  hostNote?: string;
  detailsLabel?: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation("actors");
  const selected = profiles.find((profile) => actorProfileIdentityKey(profile) === value);
  const selectedCommand = selected ? formatRuntimeCommand(selected.command) : "";
  return (
    <div className="space-y-3">
      <div>
        <label className="mb-2 block text-xs font-medium text-[var(--color-text-muted)]">
          {t("actorProfile")}
        </label>
        <SelectCombobox
          className="w-full min-h-[40px] rounded-xl border px-3 py-2 text-sm glass-input text-[var(--color-text-primary)]"
          value={value}
          onChange={onChange}
          disabled={busy || disabled}
          ariaLabel={t("actorProfile")}
          items={[
            { value: "", label: busy ? t("loadingProfiles") : t("selectActorProfile") },
            ...profiles.map((profile) => ({
              value: actorProfileIdentityKey(profile),
              label: `${profile.name || profile.id} · ${runtimeProfileScopeLabel(profile, t)}`,
            })),
          ]}
          searchable
        />
        <OpenCodeManagedModelHint runtime={selected?.runtime} />
        {!busy && profiles.length === 0 && emptyHint ? (
          <p className="mt-1.5 text-[10px] leading-4 text-[var(--color-text-muted)]">{emptyHint}</p>
        ) : null}
      </div>

      {selected && detailsLabel ? (
        <Surface
          className="px-3 py-2.5 text-xs text-[var(--color-text-secondary)]"
          variant="subtle"
          radius="md"
          padding="none"
        >
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate font-medium">{selected.name || selected.id}</div>
              <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[var(--color-text-muted)]">
                <span>{runtimeProfileScopeLabel(selected, t)}</span>
                <span aria-hidden="true">·</span>
                <span>{RUNTIME_INFO[String(selected.runtime)]?.label || selected.runtime}</span>
              </div>
            </div>
          </div>
          {selectedCommand || hostNote ? (
            <details className="mt-2 border-t border-[var(--glass-border-subtle)] pt-2">
              <summary className="cursor-pointer select-none text-[11px] font-medium text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45">
                {detailsLabel}
              </summary>
              {selectedCommand ? (
                <div className="mt-2 break-all font-mono leading-5">{selectedCommand}</div>
              ) : null}
              {hostNote ? (
                <p className="mt-2 leading-4 text-[var(--color-text-muted)]">{hostNote}</p>
              ) : null}
            </details>
          ) : null}
        </Surface>
      ) : selected ? (
        <Surface
          className="px-3 py-2 text-xs text-[var(--color-text-secondary)]"
          variant="subtle"
          radius="md"
          padding="none"
        >
          <div className="font-medium">{selected.name || selected.id}</div>
          <div className="mt-1">{runtimeProfileScopeLabel(selected, t)}</div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <span>{RUNTIME_INFO[String(selected.runtime)]?.label || selected.runtime}</span>
          </div>
          {selectedCommand ? (
            <div className="mt-1 break-all font-mono">{selectedCommand}</div>
          ) : null}
          {hostNote ? (
            <p className="mt-2 leading-4 text-[var(--color-text-muted)]">{hostNote}</p>
          ) : null}
        </Surface>
      ) : null}
    </div>
  );
}

export function RuntimeCommandControl({
  runtime,
  command,
  defaultCommand,
  useDefaultCommand,
  disabled,
  description,
  onCommandChange,
  onUseDefaultCommandChange,
}: {
  runtime: string;
  command: string;
  defaultCommand: string;
  useDefaultCommand: boolean;
  disabled?: boolean;
  description?: string;
  onCommandChange: (command: string) => void;
  onUseDefaultCommandChange: (value: boolean) => void;
}) {
  const { t } = useTranslation("actors");
  const supportsDefaultCommand = runtime !== "custom" && runtime !== "web_model";
  const showCommandEditor = runtime === "custom" || !supportsDefaultCommand || !useDefaultCommand;
  if (runtime === "web_model") return null;

  return (
    <div className="space-y-3">
      {supportsDefaultCommand ? (
        <label className="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
          <input
            type="checkbox"
            checked={useDefaultCommand}
            onChange={(event) => onUseDefaultCommandChange(event.target.checked)}
            disabled={disabled}
          />
          {t("useRuntimeDefaultCommand")}
        </label>
      ) : null}

      {showCommandEditor ? (
        <div>
          <label className="mb-2 block text-xs font-medium text-[var(--color-text-muted)]">
            {runtime === "custom" ? t("command") : t("customCommandOverride")}
          </label>
          <Input
            className="font-mono"
            value={command}
            onChange={(event) => onCommandChange(event.target.value)}
            placeholder={defaultCommand || "/path/to/custom-agent --option"}
            disabled={disabled}
            spellCheck={false}
          />
        </div>
      ) : null}

      {description ? (
        <p className="text-[10px] leading-4 text-[var(--color-text-muted)]">{description}</p>
      ) : null}
    </div>
  );
}
