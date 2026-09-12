import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchVoiceAsrProviders,
  saveVoiceAsrProvider,
  probeVoiceAsrProvider,
  type VoiceAsrProvider,
  type VoiceAsrProviderConfig,
  type VoiceAsrProviderUpdate,
} from "../../../services/api/voiceAsrProviders";
const EMPTY_SECRETS = { api_key: "", app_id: "", access_token: "" };
export function useExternalAsrProvider(
  provider: VoiceAsrProvider,
  disabled: boolean,
  onConfigured: () => void,
) {
  const { t } = useTranslation("settings");
  const [config, setConfig] = useState<VoiceAsrProviderConfig | null>(null);
  const [savedConfig, setSavedConfig] = useState<VoiceAsrProviderConfig | null>(null);
  const [secrets, setSecrets] = useState(EMPTY_SECRETS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const current = useRef(provider);
  current.current = provider;
  useEffect(() => {
    let cancelled = false;
    setConfig(null);
    setSavedConfig(null);
    setSecrets(EMPTY_SECRETS);
    setError("");
    setNotice("");
    void fetchVoiceAsrProviders()
      .then((response) => {
        if (cancelled) return;
        if (response.ok) {
          const loaded =
            response.result.providers.find((item) => item.provider === provider) || null;
          setConfig(loaded);
          setSavedConfig(loaded);
        } else setError(response.error?.message || t("assistants.externalAsr.loadFailed"));
      })
      .catch(() => {
        if (!cancelled) setError(t("assistants.externalAsr.loadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [provider, t]);
  const selected = config?.provider === provider ? config : null;
  const canProbe = Boolean(
    selected?.configured &&
    savedConfig &&
    Object.values(secrets).every((value) => !value) &&
    (["region", "workspace_id", "model", "resource_id", "auth_mode"] as const).every(
      (key) => selected[key] === savedConfig[key],
    ),
  );
  const locked = disabled || busy || !selected;
  const label = (key: string) => t(`assistants.externalAsr.${key}`);
  const update = (key: keyof VoiceAsrProviderConfig, value: string) =>
    setConfig((previous) => (previous ? { ...previous, [key]: value } : null));
  const save = async (clear = false) => {
    if (!selected || locked) return;
    setBusy(true);
    setError("");
    setNotice("");
    const patch: VoiceAsrProviderUpdate = {
      region: selected.region,
      workspace_id: selected.workspace_id,
      model: selected.model,
      resource_id: selected.resource_id,
      auth_mode: selected.auth_mode,
      ...(clear ? { clear_credentials: true } : secrets),
    };
    try {
      const response = await saveVoiceAsrProvider(provider, patch);
      if (current.current !== provider) return;
      if (!response.ok) {
        setError(response.error?.message || label("saveFailed"));
        return;
      }
      setConfig(response.result);
      setSavedConfig(response.result);
      setSecrets(EMPTY_SECRETS);
      setNotice(label(clear ? "cleared" : "saved"));
      onConfigured();
    } catch {
      if (current.current === provider) setError(label("saveFailed"));
    } finally {
      setBusy(false);
    }
  };
  const probe = async () => {
    if (locked || !canProbe) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const response = await probeVoiceAsrProvider(provider);
      if (current.current !== provider) return;
      if (response.ok) setNotice(label("connected"));
      else {
        const codes: Record<string, string> = {
          external_asr_resource_not_granted: "resourceNotGranted",
          external_asr_auth_failed: "authFailed",
          external_asr_access_denied: "accessDenied",
          external_asr_rate_limited: "rateLimited",
        };
        const key = codes[response.error?.code || ""];
        setError(key ? label(key) : response.error?.message || label("probeFailed"));
      }
    } catch {
      if (current.current === provider) setError(label("probeFailed"));
    } finally {
      setBusy(false);
    }
  };
  return {
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
  };
}
