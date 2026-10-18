import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  cancelReplyRequest,
  contextSync,
  deliverMessage,
  enableGroupCapability,
  fetchContext,
  fetchSlashCommandCapabilityState,
  fetchGroupCapabilityState,
  uninstallCapability,
  updateGroupCapabilityVisibility,
  replyMessage,
  sendCrossGroupMessage,
  sendMessage,
} from "./context";

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

  it.each([
    ["full", false],
    ["full", true],
    ["slash", false],
    ["slash", true],
  ] as const)(
    "starts a fresh %s read after a visibility write (noCache=%s)",
    async (view, noCache) => {
      vi.stubGlobal("window", { location: { search: "" } });
      let finishOld!: (response: Response) => void;
      const old = new Promise<Response>((resolve) => {
        finishOld = resolve;
      });
      let reads = 0;
      const state = (hidden: string[]) =>
        new Response(JSON.stringify({ ok: true, result: { actor_hidden_capabilities: hidden } }));
      vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
        if (String(input).endsWith("/capabilities/visibility")) {
          return new Response(JSON.stringify({ ok: true, result: {} }));
        }
        reads += 1;
        return reads === 1 ? old : state(["skill:fixture:first"]);
      });
      const fetchState = () =>
        view === "slash"
          ? fetchSlashCommandCapabilityState("g-visibility", "user", { noCache })
          : fetchGroupCapabilityState("g-visibility", "user", { noCache });
      const pending = fetchState();
      try {
        await updateGroupCapabilityVisibility("g-visibility", "skill:fixture:first", {
          hidden: true,
        });
        const refreshed = fetchState();
        expect(reads).toBe(2);
        expect(await refreshed).toMatchObject({
          ok: true,
          result: { actor_hidden_capabilities: ["skill:fixture:first"] },
        });
      } finally {
        finishOld(state([]));
        await pending;
      }
    },
  );

  it.each(["enable", "uninstall"] as const)(
    "invalidates in-flight capability-scoped reads when %s completes",
    async (action) => {
      vi.stubGlobal("window", { location: { search: "" } });
      let finishOld!: (response: Response) => void;
      const old = new Promise<Response>((resolve) => {
        finishOld = resolve;
      });
      let reads = 0;
      const state = (used: boolean) =>
        new Response(
          JSON.stringify({
            ok: true,
            result: { capability_usage: { capability_id: "skill:fixture:first", used } },
          }),
        );
      vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
        const url = String(input);
        if (url.endsWith("/capabilities/enable") || url.endsWith("/capabilities/uninstall")) {
          return new Response(
            JSON.stringify({ ok: true, result: { enabled: true, state: "ready" } }),
          );
        }
        reads += 1;
        return reads === 1 ? old : state(action === "enable");
      });

      const pending = fetchGroupCapabilityState("g-cap", "user", {
        capabilityId: "skill:fixture:first",
        noCache: true,
      });
      try {
        if (action === "enable") {
          await enableGroupCapability("g-cap", "skill:fixture:first", {
            enabled: true,
            scope: "actor",
            actorId: "peer-1",
          });
        } else {
          await uninstallCapability("g-cap", "skill:fixture:first");
        }
        const refreshed = fetchGroupCapabilityState("g-cap", "user", {
          capabilityId: "skill:fixture:first",
          noCache: true,
        });
        expect(reads).toBe(2);
        expect(await refreshed).toMatchObject({
          ok: true,
          result: {
            capability_usage: { capability_id: "skill:fixture:first", used: action === "enable" },
          },
        });
      } finally {
        finishOld(state(false));
        await pending;
      }
    },
  );

  it("invalidates a context read started while contextSync was in flight", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    let finishSync!: (response: Response) => void;
    const syncPromise = new Promise<Response>((resolve) => {
      finishSync = resolve;
    });
    let finishOldRead!: (response: Response) => void;
    const oldReadPromise = new Promise<Response>((resolve) => {
      finishOldRead = resolve;
    });
    let contextReads = 0;
    const makeContextResponse = (version: string) =>
      new Response(
        JSON.stringify({
          ok: true,
          result: { version, coordination: { brief: {}, tasks: [] }, agent_states: [] },
        }),
      );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      if (url.endsWith("/api/v1/groups/g-ctx/context") && init?.method === "POST") {
        return syncPromise;
      }
      contextReads += 1;
      return contextReads === 1 ? oldReadPromise : makeContextResponse("v2");
    });

    const syncing = contextSync("g-ctx", [{ op: "coordination.brief.update", objective: "new" }]);
    const inFlightRead = fetchContext("g-ctx", { detail: "summary" });
    finishSync(new Response(JSON.stringify({ ok: true, result: { version: "v2" } })));
    await syncing;

    try {
      const postSyncRead = fetchContext("g-ctx", { detail: "summary" });
      expect(contextReads).toBe(2);
      expect(await postSyncRead).toMatchObject({ ok: true, result: { version: "v2" } });
    } finally {
      finishOldRead(makeContextResponse("v1"));
      await inFlightRead;
    }
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
      "client-1",
      [],
      "quoted text",
      "mail",
    );

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body))).toMatchObject({
      reply_to: "event-1",
      quote_text: "quoted text",
      message_mode: "mail",
    });
  });
});

describe("message delivery control API helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("promotes the existing event for explicit recipients", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: {} })));

    await deliverMessage("g1", "event/1", ["peer-1"], true);

    expect(String(fetchMock.mock.calls[0]?.[0] || "")).toContain(
      "/api/v1/groups/g1/messages/event%2F1/deliver",
    );
    const request = fetchMock.mock.calls[0]?.[1] as RequestInit | undefined;
    expect(JSON.parse(String(request?.body))).toEqual({
      actor_ids: ["peer-1"],
      force_ambiguous: true,
    });
  });

  it("cancels reply obligations for the existing event", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: {} })));

    await cancelReplyRequest("g1", "event-1");

    expect(String(fetchMock.mock.calls[0]?.[0] || "")).toContain(
      "/api/v1/groups/g1/messages/event-1/reply-request/cancel",
    );
    const request = fetchMock.mock.calls[0]?.[1] as RequestInit | undefined;
    expect(JSON.parse(String(request?.body))).toEqual({});
  });
});

describe("cross-group attachment API helper", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("uploads files with the destination Group and reply thread", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: {} })));
    const file = new File(["png"], "chart.png", { type: "image/png" });

    const response = await sendCrossGroupMessage(
      "g1",
      "g2",
      "see chart",
      ["lead"],
      "send",
      [file],
      { replyTo: "local-parent", clientId: "client-1", remoteReplyToEventId: "remote-parent" },
    );

    expect(response.ok).toBe(true);
    const [url, init] = fetchMock.mock.calls[0] ?? [];
    expect(String(url)).toContain("/api/v1/groups/g1/send_upload");
    const form = (init as RequestInit).body as FormData;
    expect(form.get("dst_group_id")).toBe("g2");
    expect(form.get("to_json")).toBe(JSON.stringify(["lead"]));
    expect(form.get("reply_to")).toBe("local-parent");
    expect(form.get("client_id")).toBe("client-1");
    expect(form.get("remote_reply_to_event_id")).toBe("remote-parent");
    expect((form.get("files") as File).name).toBe("chart.png");
  });

  it("refuses more than 100 MiB in total before uploading", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const half = new File(["x"], "half.bin");
    Object.defineProperty(half, "size", { value: 50 * 1024 * 1024 + 1 });

    for (const send of [
      () => sendCrossGroupMessage("g1", "g2", "", ["lead"], "send", [half, half]),
      () => sendMessage("g1", "", ["user"], [half, half]),
      () => replyMessage("g1", "", ["user"], "parent", [half, half]),
    ]) {
      const response = await send();
      expect(response).toEqual({
        ok: false,
        error: { code: "attachment_too_large", message: "Attachments exceed 100 MiB in total." },
      });
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
