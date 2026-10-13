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
import { inputClass } from "./settings/types";
import { SettingsCard, SettingsRow } from "./settings/SettingsKit";
import type { ActorProfile } from "../../types";
import {
  structuredVoiceRuntimes,
  structuredVoiceRuntimeLabel,
} from "../../features/voice/voiceRuntimeOptions";

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
    <SettingsCard title={tActors("sectionRuntime")} description={runtimeHint}>
      <div className="px-5 py-3.5">
        <RuntimeConfigurationModePicker
          value={form.mode}
          disabled={form.editingDisabled}
          onChange={form.changeMode}
        />
      </div>
      <div>
        {form.mode === "profile" ? (
          <div className="px-5 py-3.5">
            <RuntimeProfilePicker
              value={form.profileIdentity}
              profiles={form.compatibleProfiles}
              busy={form.loading}
              disabled={form.editingDisabled}
              emptyHint={t("codexVoiceAnalystCompatibleProfilesEmpty")}
              detailsLabel={t("codexVoiceAnalystProfileDetails")}
              onChange={form.selectProfile}
            />
          </div>
        ) : (
          <div className="divide-y divide-[var(--glass-border-subtle)]">
            <SettingsRow
              label={tActors("runtime")}
              help={t("codexVoiceAnalystSupportedRuntimesHint")}
            >
              <div className="w-full min-w-0">
                <SelectCombobox
                  className={inputClass()}
                  ariaLabel={tActors("runtime")}
                  value={form.settings.runtime}
                  disabled={form.editingDisabled}
                  onChange={form.setRuntime}
                  items={[
                    ...(allowUnconfigured ? [{ value: "", label: tActors("selectRuntime") }] : []),
                    ...structuredVoiceRuntimes.map((runtime) => ({
                      value: runtime,
                      label: structuredVoiceRuntimeLabel(runtime),
                    })),
                  ]}
                />
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
            </SettingsRow>
            {form.settings.runtime && (
              <div className="px-5 py-3.5">
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
              </div>
            )}
          </div>
        )}
      </div>
      {form.mode === "custom" && (
        <details className="px-5 py-3.5">
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
    </SettingsCard>
  );
}
