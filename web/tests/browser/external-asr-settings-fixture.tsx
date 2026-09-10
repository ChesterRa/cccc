// Real production settings controls and API client; only HTTP responses are synthetic.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import settings from "../../src/i18n/locales/zh/settings.json";
import common from "../../src/i18n/locales/zh/common.json";
import { ExternalAsrSettings } from "../../src/components/modals/settings/ExternalAsrSettings";
import type {
  VoiceAsrProvider,
  VoiceAsrProviderConfig,
} from "../../src/services/api/voiceAsrProviders";
import "../../src/index.css";

await i18next.use(initReactI18next).init({ lng: "zh", resources: { zh: { settings, common } } });
const providers: VoiceAsrProviderConfig[] = ["bailian", "volcengine"].map((provider) => ({
  provider: provider as VoiceAsrProvider,
  region: "beijing",
  workspace_id: "",
  model: "fun-asr-realtime",
  resource_id: "volc.seedasr.sauc.duration",
  auth_mode: "api_key",
  has_api_key: false,
  has_app_id: false,
  has_access_token: false,
  configured: false,
}));
const requests: { url: string; method: string; body: Record<string, unknown> }[] = [];
Object.assign(window, { externalAsrFixture: { requests, providers } });
window.fetch = async (input, options) => {
  const url = String(input);
  const method = options?.method || "GET";
  if (!url.startsWith("/api/v1/voice/asr/providers")) throw new Error(`Unexpected request: ${url}`);
  const body = options?.body ? JSON.parse(String(options.body)) : {};
  requests.push({ url, method, body });
  const provider = providers.find((item) => url.includes(`/${item.provider}`));
  if (method === "GET") return Response.json({ ok: true, result: { providers } });
  if (!provider) throw new Error("missing provider");
  if (method === "PUT") {
    for (const key of ["region", "workspace_id", "model", "resource_id", "auth_mode"] as const) {
      if (body[key] !== undefined) Object.assign(provider, { [key]: body[key] });
    }
    for (const key of ["api_key", "app_id", "access_token"] as const) {
      if (body.clear_credentials) provider[`has_${key}`] = false;
      else if (body[key]) provider[`has_${key}`] = true;
    }
    provider.configured =
      provider.auth_mode === "app_token"
        ? provider.has_app_id && provider.has_access_token
        : provider.has_api_key;
    return Response.json({ ok: true, result: provider });
  }
  if (method === "POST")
    return Response.json({
      ok: true,
      result: { provider: provider.provider, connected: true, model_id: "fixture" },
    });
  throw new Error("unexpected method");
};
export function Fixture() {
  const [provider, setProvider] = useState<VoiceAsrProvider>("bailian");
  return (
    <main className="mx-auto max-w-3xl p-4">
      <ExternalAsrSettings
        provider={provider}
        onProviderChange={setProvider}
        disabled={false}
        onConfigured={() => undefined}
      />
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
