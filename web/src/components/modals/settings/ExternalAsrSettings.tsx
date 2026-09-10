import { GroupCombobox } from "../../GroupCombobox";
import { Input } from "../../ui/input";
import { Button } from "../../ui/button";
import type {
  VoiceAsrProvider,
  VoiceAsrProviderConfig,
} from "../../../services/api/voiceAsrProviders";
import { useExternalAsrProvider } from "./useExternalAsrProvider";

interface Props {
  provider: VoiceAsrProvider;
  onProviderChange: (provider: VoiceAsrProvider) => void;
  disabled: boolean;
  onConfigured: () => void;
}

export function ExternalAsrSettings({ provider, onProviderChange, disabled, onConfigured }: Props) {
  const {
    selected,
    secrets,
    busy,
    error,
    notice,
    locked,
    canProbe,
    label,
    t,
    update,
    save,
    probe,
    setSecrets,
  } = useExternalAsrProvider(provider, disabled, onConfigured);
  const combo = (
    key: keyof VoiceAsrProviderConfig,
    title: string,
    items: { value: string; label: string }[],
  ) => (
    <div>
      <div className="mb-1 text-xs text-[var(--color-text-secondary)]">{label(title)}</div>
      <GroupCombobox
        placeholder={label(title)}
        searchPlaceholder={label(title)}
        emptyText={t("common:noResults")}
        ariaLabel={label(title)}
        value={String(selected?.[key] || "")}
        items={items}
        onChange={(value) => update(key, value)}
        disabled={locked}
        searchable={false}
        matchTriggerWidth
        triggerClassName="min-h-[44px] w-full"
      />
    </div>
  );
  const secretField = (key: keyof typeof secrets, title: string, configured: boolean) => (
    <label className="block space-y-1 text-xs text-[var(--color-text-secondary)]">
      <span>{label(title)}</span>
      <Input
        type="password"
        autoComplete="new-password"
        spellCheck={false}
        value={secrets[key]}
        disabled={locked}
        placeholder={label(configured ? "keepSecret" : "enterSecret")}
        onChange={(event) => setSecrets((previous) => ({ ...previous, [key]: event.target.value }))}
      />
    </label>
  );
  return (
    <section
      className="mt-4 space-y-4 rounded-xl border border-[var(--glass-border-subtle)] p-4"
      aria-label={label("title")}
    >
      <p className="text-xs leading-5 text-[var(--color-text-secondary)]">{label("hint")}</p>
      <GroupCombobox
        placeholder={label("provider")}
        searchPlaceholder={label("provider")}
        emptyText={t("common:noResults")}
        ariaLabel={label("provider")}
        value={provider}
        disabled={disabled || busy}
        searchable={false}
        matchTriggerWidth
        triggerClassName="min-h-[44px] w-full"
        items={[
          { value: "bailian", label: label("bailian") },
          { value: "volcengine", label: label("volcengine") },
        ]}
        onChange={(value) => onProviderChange(value as VoiceAsrProvider)}
      />
      {selected && (
        <>
          <p className="text-xs text-[var(--color-text-secondary)]">
            {label(selected.configured ? "configured" : "notConfigured")}
          </p>
          <div className="grid gap-3 sm:grid-cols-2">
            {provider === "bailian" ? (
              <>
                {combo("region", "region", [
                  { value: "beijing", label: label("beijing") },
                  { value: "singapore", label: label("singapore") },
                ])}
                {combo("model", "model", [
                  { value: "fun-asr-realtime", label: "Fun-ASR Realtime" },
                  { value: "paraformer-realtime-v2", label: "Paraformer Realtime v2" },
                ])}
                <label className="block space-y-1 text-xs text-[var(--color-text-secondary)]">
                  <span>{label("workspace")}</span>
                  <Input
                    value={selected.workspace_id}
                    disabled={locked}
                    onChange={(event) => update("workspace_id", event.target.value)}
                  />
                </label>
                {secretField("api_key", "apiKey", selected.has_api_key)}
              </>
            ) : (
              <>
                {combo("auth_mode", "authMode", [
                  { value: "api_key", label: label("apiKeyAuth") },
                  { value: "app_token", label: label("legacyAuth") },
                ])}
                {combo("resource_id", "resource", [
                  { value: "volc.seedasr.sauc.duration", label: label("v2Duration") },
                  { value: "volc.seedasr.sauc.concurrent", label: label("v2Concurrent") },
                  { value: "volc.bigasr.sauc.duration", label: label("v1Duration") },
                  { value: "volc.bigasr.sauc.concurrent", label: label("v1Concurrent") },
                ])}
                {selected.auth_mode === "app_token" ? (
                  <>
                    {secretField("app_id", "appId", selected.has_app_id)}
                    {secretField("access_token", "accessToken", selected.has_access_token)}
                  </>
                ) : (
                  secretField("api_key", "apiKey", selected.has_api_key)
                )}
              </>
            )}
          </div>
          {provider === "volcengine" && (
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {label("credentialHint")} {label("automaticLanguage")}
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              className="min-h-[44px]"
              type="button"
              disabled={locked}
              onClick={() => void save()}
            >
              {label("save")}
            </Button>
            <Button
              className="min-h-[44px]"
              type="button"
              variant="outline"
              disabled={locked || !canProbe}
              onClick={() => void probe()}
            >
              {label("probe")}
            </Button>
            <Button
              className="min-h-[44px]"
              type="button"
              variant="ghost"
              disabled={
                locked ||
                !(selected.has_api_key || selected.has_app_id || selected.has_access_token)
              }
              onClick={() => void save(true)}
            >
              {label("clear")}
            </Button>
          </div>
          <p className="text-xs leading-5 text-[var(--color-text-tertiary)]">{label("scope")}</p>
        </>
      )}
      {error && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {notice}
        </p>
      )}
    </section>
  );
}
