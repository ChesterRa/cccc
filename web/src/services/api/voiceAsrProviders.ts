import { apiJson } from "./base";

export type VoiceAsrProvider = "bailian" | "volcengine";
export interface VoiceAsrProviderConfig {
  provider: VoiceAsrProvider;
  region: "beijing" | "singapore";
  workspace_id: string;
  model: string;
  resource_id: string;
  auth_mode: "api_key" | "app_token";
  has_api_key: boolean;
  has_app_id: boolean;
  has_access_token: boolean;
  configured: boolean;
}
export type VoiceAsrProviderUpdate = Partial<
  Pick<VoiceAsrProviderConfig, "region" | "workspace_id" | "model" | "resource_id" | "auth_mode">
> & { api_key?: string; app_id?: string; access_token?: string; clear_credentials?: boolean };
export const fetchVoiceAsrProviders = () =>
  apiJson<{ providers: VoiceAsrProviderConfig[] }>("/api/v1/voice/asr/providers");
export const saveVoiceAsrProvider = (provider: VoiceAsrProvider, patch: VoiceAsrProviderUpdate) =>
  apiJson<VoiceAsrProviderConfig>(`/api/v1/voice/asr/providers/${provider}`, {
    method: "PUT",
    body: JSON.stringify(patch),
  });
export const probeVoiceAsrProvider = (provider: VoiceAsrProvider) =>
  apiJson<{ provider: VoiceAsrProvider; model_id: string; connected: boolean }>(
    `/api/v1/voice/asr/providers/${provider}/probe`,
    { method: "POST", body: "{}" },
  );
