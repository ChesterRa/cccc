import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { fetchSlashCommandCapabilityState, replyMessage } from "./context";

describe("capability state API helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("requests the compact slash-command capability view", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              group_id: "g1",
              actor_id: "user",
              view: "slash_commands",
              dynamic_tools: [],
              active_capsule_skills: [],
              actor_hidden_capabilities: [],
            },
          }),
        ),
      );

    await fetchSlashCommandCapabilityState("g1", "user", { noCache: true });

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/capabilities/state");
    expect(url).toContain("actor_id=user");
    expect(url).toContain("view=slash_commands");
  });
});

describe("reply message API helper", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("persists the quoted message snapshot with the reply", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: {} })));

    await replyMessage(
      "g1",
      "reply",
      ["actor"],
      "event-1",
      undefined,
      "normal",
      false,
      "client-1",
      [],
      "quoted text",
    );

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body))).toMatchObject({
      reply_to: "event-1",
      quote_text: "quoted text",
    });
  });
});
