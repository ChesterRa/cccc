import { useTranslation } from "react-i18next";
import { ActorSecretManager } from "./ActorSecretManager";
import type { ActorSecretChanges } from "./actorSecretManagerModel";
import {
  RuntimeCommandControl,
  RuntimeConfigurationModePicker,
  RuntimeProfilePicker,
  OpenCodeManagedModelHint,
} from "./RuntimeProfileControls";
import { SelectCombobox } from "../SelectCombobox";
import { inputClass, labelClass } from "./settings/types";
import { RUNTIME_INFO, type ActorProfile } from "../../types";

export type StructuredRuntimeForm = {
  mode: "custom" | "profile";
  settings: { runtime: string; command: string; profile_id: string };
  compatibleProfiles: ActorProfile[];
  profileIdentity: string;
  loading: boolean;
  editingDisabled: boolean;
  changeMode: (mode: "custom" | "profile") => void;
  selectProfile: (identity: string) => void;
  setRuntime: (runtime: string) => void;
  defaultCommand: string;
  useDefaultCommand: boolean;
  setCommand: (command: string) => void;
  setUseDefaultCommand: (useDefault: boolean) => void;
  environmentKeys: string[];
  environmentChanges: ActorSecretChanges;
  environmentRefreshing: boolean;
  settingsLoadFailed: boolean;
  refreshEnvironment: () => Promise<unknown>;
  setEnvironmentChanges: (changes: ActorSecretChanges) => void;
};

export function StructuredRuntimeSettingsFields({
  form,
  runtimeHint,
  commandHint,
  allowUnconfigured = false,
}: {
  form: StructuredRuntimeForm;
  runtimeHint: string;
  commandHint: string;
  allowUnconfigured?: boolean;
}) {
  const { t } = useTranslation("modals");
  const { t: tActors } = useTranslation("actors");
  return (
    <div className="space-y-5">
      <div>
        <h3 className="text-sm font-semibold">{tActors("sectionRuntime")}</h3>
        <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">{runtimeHint}</p>
        <div className="mt-4">
          <RuntimeConfigurationModePicker
            value={form.mode}
            disabled={form.editingDisabled}
            onChange={form.changeMode}
          />
        </div>
        <div className="mt-4">
          {form.mode === "profile" ? (
            <RuntimeProfilePicker
              value={form.profileIdentity}
              profiles={form.compatibleProfiles}
              busy={form.loading}
              disabled={form.editingDisabled}
              emptyHint={t("codexVoiceAnalystCompatibleProfilesEmpty")}
              detailsLabel={t("codexVoiceAnalystProfileDetails")}
              onChange={form.selectProfile}
            />
          ) : (
            <div className="space-y-4">
              <div>
                <label className={labelClass()}>{tActors("runtime")}</label>
                <SelectCombobox
                  className={inputClass()}
                  ariaLabel={tActors("runtime")}
                  value={form.settings.runtime}
                  disabled={form.editingDisabled}
                  onChange={form.setRuntime}
                  items={[
                    ...(allowUnconfigured ? [{ value: "", label: tActors("selectRuntime") }] : []),
                    ...(["codex", "claude", "grok", "opencode", "kilo"] as const).map(
                      (runtime) => ({ value: runtime, label: RUNTIME_INFO[runtime].label }),
                    ),
                    { value: "antigravity", label: "Antigravity (ACP)" },
                    { value: "copilot", label: "GitHub Copilot (ACP)" },
                    { value: "devin", label: "Devin CLI (ACP)" },
                    { value: "cursor", label: "Cursor (ACP)" },
                  ]}
                />
                <p className="mt-1.5 text-xs leading-5 text-[var(--color-text-muted)]">
                  {t("codexVoiceAnalystSupportedRuntimesHint")}
                </p>
                <OpenCodeManagedModelHint runtime={form.settings.runtime} />
                {form.settings.runtime === "antigravity" && (
                  <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
                    {tActors("antigravityMode.hint")}
                    <br />
                    <code className="break-all">
                      cccc setup --runtime antigravity --runtime-mode acp --login
                    </code>
                  </p>
                )}
              </div>
              {form.settings.runtime && (
                <RuntimeCommandControl
                  runtime={form.settings.runtime}
                  command={form.settings.command}
                  defaultCommand={form.defaultCommand}
                  useDefaultCommand={form.useDefaultCommand}
                  disabled={form.editingDisabled}
                  description={commandHint}
                  onCommandChange={form.setCommand}
                  onUseDefaultCommandChange={form.setUseDefaultCommand}
                />
              )}
            </div>
          )}
        </div>
      </div>
      {form.mode === "custom" && (
        <details className="border-t border-[var(--glass-border-subtle)] pt-4">
          <summary className="cursor-pointer select-none text-sm font-semibold">
            {tActors("sectionAdvanced")}
          </summary>
          <div className="mt-4">
            <p className="mb-2 text-xs text-[var(--color-text-secondary)]">
              {tActors("secretsSection")}
            </p>
            <ActorSecretManager
              keys={form.environmentKeys}
              masks={{}}
              changes={form.environmentChanges}
              loading={form.environmentRefreshing}
              keysLoadFailed={form.settingsLoadFailed}
              disabled={form.editingDisabled}
              onRefresh={() => void form.refreshEnvironment()}
              onChangesChange={form.setEnvironmentChanges}
            />
          </div>
        </details>
      )}
    </div>
  );
}
