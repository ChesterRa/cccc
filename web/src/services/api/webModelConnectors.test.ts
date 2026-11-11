import { afterEach, expect, it, vi } from "vite-plus/test";
import { createWebModelConnector, rotateWebModelConnector } from "./webModel";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("sends rotation intent only from the explicit rotation operation", async () => {
  vi.stubGlobal("window", { location: { search: "" } });
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  await createWebModelConnector("chatgpt_web");
  await createWebModelConnector("chatgpt_web", { createOnly: true });
  await rotateWebModelConnector("grok_web");
  expect(
    fetch.mock.calls.map(([url, init]) => ({
      url,
      method: init?.method,
      body: JSON.parse(String(init?.body)),
    })),
  ).toEqual([
    { url: "/api/v1/web-model/connectors", method: "POST", body: { provider: "chatgpt_web" } },
    {
      url: "/api/v1/web-model/connectors",
      method: "POST",
      body: { provider: "chatgpt_web", create_only: true },
    },
    {
      url: "/api/v1/web-model/connectors",
      method: "POST",
      body: { provider: "grok_web", rotate: true },
    },
  ]);
});
