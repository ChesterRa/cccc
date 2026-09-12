// Production settings + API client; only HTTP is replaced. No actors or cloud requests run.
import { createRoot } from "react-dom/client";
import i18next from "../../src/i18n";
import { AssistantsTab } from "../../src/components/modals/settings/AssistantsTab";
import "../../src/index.css";

await i18next.changeLanguage("zh");
const assistant = {
  assistant_id: "voice_secretary",
  enabled: true,
  config: {
    recognition_backend: "browser_asr",
    external_asr_provider: "bailian",
    auto_document_max_window_seconds: 300,
  },
};
const fixture = { assistant, writes: [] as Record<string, unknown>[], failNext: false };
Object.assign(window, { voiceSettingsFixture: fixture });
window.fetch = async (input, options) => {
  const url = String(input);
  if (options?.method === "PUT" && url.endsWith("/settings")) {
    const body = JSON.parse(String(options.body));
    fixture.writes.push(body);
    if (fixture.failNext) {
      fixture.failNext = false;
      return Response.json({ ok: false, error: { code: "save_failed", message: "测试保存失败" } });
    }
    Object.assign(assistant.config, body.config);
    if (typeof body.enabled === "boolean") assistant.enabled = body.enabled;
    return Response.json({ ok: true, result: { group_id: "autosave", assistant } });
  }
  if (url.includes("/voice/asr/providers"))
    return Response.json({
      ok: true,
      result: {
        providers: ["bailian", "volcengine"].map((provider) => ({
          provider,
          configured: false,
          region: "beijing",
          workspace_id: "",
          model: "fun-asr-realtime",
          resource_id: "volc.bigasr.sauc.duration",
          auth_mode: "app_token",
        })),
      },
    });
  if (url.includes("/assistants"))
    return Response.json({
      ok: true,
      result: {
        group_id: "autosave",
        assistants: [assistant],
        assistants_by_id: { voice_secretary: assistant },
      },
    });
  if (url.includes("/prompts"))
    return Response.json({ ok: true, result: { content: "", help: { content: "" } } });
  throw new Error(`Unexpected HTTP: ${url}`);
};
createRoot(document.getElementById("root")!).render(
  <AssistantsTab groupId="autosave" isActive isDark={false} busy={false} />,
);
