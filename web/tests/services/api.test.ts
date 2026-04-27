import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const fetchMock = vi.fn();

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeStorage() {
  const data = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, String(value));
    }),
    removeItem: vi.fn((key: string) => {
      data.delete(key);
    }),
    clear: vi.fn(() => {
      data.clear();
    }),
  };
}

const sessionStorageMock = makeStorage();

vi.stubGlobal("fetch", fetchMock);
vi.stubGlobal("window", {
  location: { search: "", protocol: "http:", host: "localhost" },
});
vi.stubGlobal("sessionStorage", sessionStorageMock);

describe("api error normalization", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("window", {
      location: { search: "", protocol: "http:", host: "localhost" },
    });
    vi.stubGlobal("sessionStorage", sessionStorageMock);
  });

  it("keeps regular API errors unchanged", async () => {
    const { formatApiErrorMessage } = await import("../../src/services/api");

    expect(
      formatApiErrorMessage({
        code: "permission_denied",
        message: "permission denied",
      })
    ).toBe("permission denied");
  });

  it("summarizes daemon transport diagnostics into the message", async () => {
    const { formatApiErrorMessage } = await import("../../src/services/api");

    expect(
      formatApiErrorMessage({
        code: "daemon_unavailable",
        message: "ccccd unavailable",
        details: {
          transport: "tcp",
          endpoint: { host: "127.0.0.1", port: 9001 },
          phase: "connect",
          reason: "os_error",
        },
      })
    ).toBe("ccccd unavailable · tcp 127.0.0.1:9001 · connect os error");
  });

  it("normalizes daemon_unavailable messages from response bodies", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: false,
            error: {
              code: "daemon_unavailable",
              message: "ccccd unavailable",
              details: {
                transport: "unix",
                endpoint: { path: "/tmp/ccccd.sock" },
                phase: "read",
                reason: "timeout",
              },
            },
          }),
          {
            status: 503,
            headers: { "content-type": "application/json" },
          }
        )
      )
    );

    const { apiJson } = await import("../../src/services/api");
    const resp = await apiJson("/api/v1/ping");

    expect(resp.ok).toBe(false);
    if (resp.ok) {
      throw new Error("expected error response");
    }
    expect(resp.error.message).toBe("ccccd unavailable · unix /tmp/ccccd.sock · read timeout");
    expect(resp.error.details).toEqual({
      transport: "unix",
      endpoint: { path: "/tmp/ccccd.sock" },
      phase: "read",
      reason: "timeout",
    });
  });
});

describe("api.fetchActors", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("defaults to pure-read actors without unread query", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { actors: [] } }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchActors("g-demo");

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/actors",
      expect.objectContaining({
        headers: expect.objectContaining({ "content-type": "application/json" }),
      }),
    );
  });

  it("keeps explicit unread requests on the slow path", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { actors: [] } }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchActors("g-demo", true);

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/actors?include_unread=true",
      expect.objectContaining({
        headers: expect.objectContaining({ "content-type": "application/json" }),
      }),
    );
  });
});

describe("api.fetchPresentation", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("normalizes the presentation snapshot into four stable slots", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            presentation: {
              v: 1,
              updated_at: "2026-03-21T00:00:00Z",
              highlight_slot_id: "slot-2",
              slots: [
                {
                  slot_id: "slot-2",
                  index: 2,
                  card: {
                    slot_id: "slot-2",
                    title: "Report",
                    card_type: "table",
                    published_by: "peer-1",
                    published_at: "2026-03-21T00:00:00Z",
                    content: {
                      mode: "inline",
                      table: {
                        columns: ["name"],
                        rows: [["demo"]],
                      },
                    },
                  },
                },
              ],
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchPresentation("g-demo");

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.presentation.highlight_slot_id).toBe("slot-2");
    expect(resp.result.presentation.slots).toHaveLength(4);
    expect(resp.result.presentation.slots[0]?.slot_id).toBe("slot-1");
    expect(resp.result.presentation.slots[1]?.card?.title).toBe("Report");
    expect(resp.result.presentation.slots[1]?.card?.content.table?.rows).toEqual([["demo"]]);
  });

  it("builds token-aware asset urls for presentation slots", async () => {
    sessionStorageMock.setItem("cccc_dev_token", "dev-token");
    const api = await import("../../src/services/api");
    expect(api.getPresentationAssetUrl("g-demo", "slot-4")).toBe(
      "/api/v1/groups/g-demo/presentation/slots/slot-4/asset?token=dev-token"
    );
    expect(api.getPresentationAssetUrl("g-demo", "slot-4", "tick-2")).toBe(
      "/api/v1/groups/g-demo/presentation/slots/slot-4/asset?token=dev-token&v=tick-2"
    );
  });

  it("builds token-aware blob urls only for group-scoped blob paths", async () => {
    sessionStorageMock.setItem("cccc_dev_token", "dev-token");
    const api = await import("../../src/services/api");
    expect(api.getGroupBlobUrl("g-demo", "state/blobs/sha256_demo.jpg")).toBe(
      "/api/v1/groups/g-demo/blobs/sha256_demo.jpg?token=dev-token"
    );
    expect(api.getGroupBlobUrl("g-demo", "workspace/demo.jpg")).toBe("");
  });

  it("starts a browser-surface session and normalizes its state", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            browser_surface: {
              active: true,
              state: "ready",
              message: "Browser surface ready.",
              strategy: "playwright_chromium_cdp",
              url: "http://127.0.0.1:3000",
              width: 1440,
              height: 900,
              last_frame_seq: 2,
              controller_attached: false,
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.startPresentationBrowserSurfaceSession("g-demo", {
      slotId: "slot-2",
      url: "http://127.0.0.1:3000",
      width: 1440,
      height: 900,
    });

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.browser_surface.state).toBe("ready");
    expect(resp.result.browser_surface.strategy).toBe("playwright_chromium_cdp");
    expect(resp.result.browser_surface.width).toBe(1440);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/browser_surface/session",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          by: "user",
          slot: "slot-2",
          url: "http://127.0.0.1:3000",
          width: 1440,
          height: 900,
        }),
      }),
    );
  });

  it("builds a token-aware websocket url for browser-surface streaming", async () => {
    sessionStorageMock.setItem("cccc_dev_token", "dev-token");
    vi.stubGlobal("window", {
      location: { search: "", protocol: "https:", host: "cccc.test" },
    });
    const api = await import("../../src/services/api");
    expect(api.getPresentationBrowserSurfaceWebSocketUrl("g-demo", "slot-3")).toBe(
      "wss://cccc.test/api/v1/groups/g-demo/presentation/browser_surface/ws?slot=slot-3&token=dev-token"
    );
  });

  it("requests browser-surface session info for a specific slot", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            browser_surface: {
              active: false,
              state: "idle",
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchPresentationBrowserSurfaceSession("g-demo", "slot-1");

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/browser_surface/session?slot=slot-1",
      expect.objectContaining({
        headers: expect.objectContaining({ "content-type": "application/json" }),
      }),
    );
  });

  it("uploads a presentation ref snapshot and normalizes the stored blob metadata", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            snapshot: {
              path: "state/blobs/sha256_demo.jpg",
              mime_type: "image/jpeg",
              bytes: 3210,
              sha256: "sha256_demo",
              width: 1440,
              height: 900,
              captured_at: "2026-03-22T12:00:00Z",
              source: "browser_surface",
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const file = new File(["fake-image"], "snapshot.jpg", { type: "image/jpeg" });
    const resp = await api.uploadPresentationReferenceSnapshot("g-demo", {
      slotId: "slot-3",
      file,
      source: "browser_surface",
      capturedAt: "2026-03-22T12:00:00Z",
      width: 1440,
      height: 900,
    });

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.snapshot.path).toBe("state/blobs/sha256_demo.jpg");
    expect(resp.result.snapshot.width).toBe(1440);
    const [url, requestInit] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("/api/v1/groups/g-demo/presentation/ref_snapshot");
    expect(requestInit).toEqual(
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
    const form = requestInit.body as FormData;
    expect(form.get("slot")).toBe("slot-3");
    expect(form.get("source")).toBe("browser_surface");
    expect(form.get("captured_at")).toBe("2026-03-22T12:00:00Z");
    expect(form.get("width")).toBe("1440");
    expect(form.get("height")).toBe("900");
    expect(form.get("file")).toBe(file);
  });

  it("publishes a presentation URL on the JSON endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            slot_id: "slot-2",
            card: {
              slot_id: "slot-2",
              title: "Dashboard",
              card_type: "web_preview",
              published_by: "user",
              published_at: "2026-03-21T00:00:00Z",
              content: {
                mode: "reference",
                url: "https://example.com/dashboard",
              },
            },
            presentation: {
              v: 1,
              highlight_slot_id: "slot-2",
              slots: [
                {
                  slot_id: "slot-2",
                  index: 2,
                  card: {
                    slot_id: "slot-2",
                    title: "Dashboard",
                    card_type: "web_preview",
                    published_by: "user",
                    published_at: "2026-03-21T00:00:00Z",
                    content: {
                      mode: "reference",
                      url: "https://example.com/dashboard",
                    },
                  },
                },
              ],
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.publishPresentationUrl("g-demo", {
      slotId: "slot-2",
      url: "https://example.com/dashboard",
      title: "Dashboard",
    });

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.slot_id).toBe("slot-2");
    expect(resp.result.card?.card_type).toBe("web_preview");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/publish",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          by: "user",
          slot: "slot-2",
          url: "https://example.com/dashboard",
          title: "Dashboard",
          summary: "",
        }),
      }),
    );
  });

  it("publishes a local file on the upload endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            slot_id: "slot-1",
            card: {
              slot_id: "slot-1",
              title: "notes.md",
              card_type: "markdown",
              published_by: "user",
              published_at: "2026-03-21T00:00:00Z",
              content: {
                mode: "inline",
                markdown: "# notes",
              },
            },
            presentation: {
              v: 1,
              highlight_slot_id: "slot-1",
              slots: [
                {
                  slot_id: "slot-1",
                  index: 1,
                  card: {
                    slot_id: "slot-1",
                    title: "notes.md",
                    card_type: "markdown",
                    published_by: "user",
                    published_at: "2026-03-21T00:00:00Z",
                    content: {
                      mode: "inline",
                      markdown: "# notes",
                    },
                  },
                },
              ],
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const file = new File(["# notes"], "notes.md", { type: "text/markdown" });
    const resp = await api.publishPresentationUpload("g-demo", {
      slotId: "slot-1",
      file,
    });

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.card?.title).toBe("notes.md");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/publish_upload",
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
  });

  it("publishes a workspace file on the JSON endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            slot_id: "slot-3",
            card: {
              slot_id: "slot-3",
              title: "report.md",
              card_type: "markdown",
              published_by: "user",
              published_at: "2026-03-21T00:00:00Z",
              content: {
                mode: "workspace_link",
                workspace_rel_path: "docs/report.md",
                mime_type: "text/markdown",
                file_name: "report.md",
              },
            },
            presentation: {
              v: 1,
              highlight_slot_id: "slot-3",
              slots: [
                {
                  slot_id: "slot-3",
                  index: 3,
                  card: {
                    slot_id: "slot-3",
                    title: "report.md",
                    card_type: "markdown",
                    published_by: "user",
                    published_at: "2026-03-21T00:00:00Z",
                    content: {
                      mode: "workspace_link",
                      workspace_rel_path: "docs/report.md",
                    },
                  },
                },
              ],
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.publishPresentationWorkspace("g-demo", {
      slotId: "slot-3",
      path: "docs/report.md",
      title: "report.md",
    });

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.card?.content.workspace_rel_path).toBe("docs/report.md");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/publish_workspace",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          by: "user",
          slot: "slot-3",
          path: "docs/report.md",
          title: "report.md",
          summary: "",
        }),
      }),
    );
  });

  it("loads workspace listing for presentation pinning", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            root_path: "/workspace/demo",
            path: "docs",
            parent: "",
            items: [
              { name: "report.md", path: "docs/report.md", is_dir: false, mime_type: "text/markdown" },
              { name: "assets", path: "docs/assets", is_dir: true },
            ],
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchPresentationWorkspaceListing("g-demo", "docs");

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.root_path).toBe("/workspace/demo");
    expect(resp.result.path).toBe("docs");
    expect(resp.result.items[0]?.path).toBe("docs/report.md");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/workspace/list?path=docs",
      expect.objectContaining({
        headers: expect.objectContaining({ "content-type": "application/json" }),
      }),
    );
  });

  it("clears a presentation slot on the mutation endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g-demo",
            cleared_slots: ["slot-4"],
            presentation: {
              v: 1,
              highlight_slot_id: "",
              slots: [],
            },
          },
        }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.clearPresentationSlot("g-demo", "slot-4");

    expect(resp.ok).toBe(true);
    if (!resp.ok) return;
    expect(resp.result.cleared_slots).toEqual(["slot-4"]);
    expect(resp.result.presentation.slots).toHaveLength(4);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/presentation/clear",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          by: "user",
          slot: "slot-4",
        }),
      }),
    );
  });
});

describe("api assistant voice model helpers", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("normalizes assistant state service models", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({
        ok: true,
        result: {
          group_id: "g-demo",
          assistant: {
            assistant_id: "voice_secretary",
            kind: "voice_secretary",
            enabled: true,
            lifecycle: "idle",
            health: {
              service: {
                selected_model_id: "mock_asr",
                managed_model: {
                  model_id: "mock_asr",
                  status: "ready",
                  command_ready: true,
                },
              },
            },
            config: {
              recognition_backend: "assistant_service_local_asr",
              service_model_id: "mock_asr",
            },
          },
          service_models: [
            {
              model_id: "mock_asr",
              title: "Mock ASR",
              status: "ready",
              command_ready: true,
              artifacts: [{ path: "adapter.py", size_bytes: 1234 }],
            },
          ],
        },
      }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.fetchAssistant("g-demo", "voice_secretary");

    expect(resp.ok).toBe(true);
    if (!resp.ok) throw new Error("expected ok response");
    expect(resp.result.service_models_by_id?.mock_asr?.status).toBe("ready");
    expect(resp.result.service_models?.[0]?.artifacts?.[0]?.size_bytes).toBe(1234);
    expect((resp.result.assistant?.health?.service as Record<string, unknown>)?.selected_model_id).toBe("mock_asr");
  });

  it("calls the install model endpoint and clears assistant cache", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({
        ok: true,
        result: {
          group_id: "g-demo",
          assistant: {
            assistant_id: "voice_secretary",
            kind: "voice_secretary",
            enabled: true,
            lifecycle: "idle",
          },
          model: {
            model_id: "mock_asr",
            status: "ready",
          },
        },
      }),
    });

    const api = await import("../../src/services/api");
    const resp = await api.installVoiceAssistantModel("g-demo", { modelId: "mock_asr", by: "user" });

    expect(resp.ok).toBe(true);
    if (!resp.ok) throw new Error("expected ok response");
    expect(resp.result.model?.model_id).toBe("mock_asr");
    expect(resp.result.model?.status).toBe("ready");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/assistants/voice_secretary/models/install",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ model_id: "mock_asr", by: "user", background: false }),
      }),
    );
  });
});

describe("api.message refs", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("sends structured refs on the JSON send path", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { event_id: "evt-1" } }),
    });

    const api = await import("../../src/services/api");
    const refs = [{ kind: "presentation_ref", slot_id: "slot-2", locator: { viewer_scroll_top: 240 } }];
    await api.sendMessage("g-demo", "please review", ["worker-1"], undefined, "normal", false, "client-1", refs);

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/send",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          text: "please review",
          by: "user",
          to: ["worker-1"],
          path: "",
          priority: "normal",
          reply_required: false,
          client_id: "client-1",
          refs,
        }),
      }),
    );
  });

  it("sends tracked delegation payloads through the daemon endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { task_id: "T001" } }),
    });

    const api = await import("../../src/services/api");
    const refs = [{ kind: "presentation_ref", slot_id: "slot-2" }];
    await api.trackedSendMessage("g-demo", {
      title: "Review routing",
      text: "Please review routing.",
      to: ["reviewer"],
      outcome: "Review evidence reported.",
      checklist: [{ text: "Inspect code" }],
      idempotency_key: "req-1",
      refs,
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/g-demo/tracked_send",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          title: "Review routing",
          text: "Please review routing.",
          by: "user",
          to: ["reviewer"],
          outcome: "Review evidence reported.",
          checklist: [{ text: "Inspect code" }],
          assignee: "",
          waiting_on: "actor",
          handoff_to: "",
          notes: "",
          priority: "normal",
          reply_required: true,
          idempotency_key: "req-1",
          refs,
        }),
      }),
    );
  });

  it("sends structured refs on the upload reply path", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { event_id: "evt-2" } }),
    });

    const api = await import("../../src/services/api");
    const refs = [{ kind: "presentation_ref", slot_id: "slot-3", locator: { url: "http://127.0.0.1:3000" } }];
    const file = new File(["hello"], "note.txt", { type: "text/plain" });
    await api.replyMessage("g-demo", "see attached", ["worker-2"], "evt-parent", [file], "attention", true, "client-2", refs);

    const [url, requestInit] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("/api/v1/groups/g-demo/reply_upload");
    expect(requestInit).toEqual(
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
    const form = requestInit.body as FormData;
    expect(form.get("reply_to")).toBe("evt-parent");
    expect(form.get("reply_required")).toBe("true");
    expect(form.get("client_id")).toBe("client-2");
    expect(form.get("refs_json")).toBe(JSON.stringify(refs));
    expect(form.get("files")).toBe(file);
  });
});

describe("blueprint api entrypoints", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("keeps create-group blueprint import on the form endpoint", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { group_id: "g-new" } }),
    });

    const api = await import("../../src/services/api");
    const file = new File(["title: demo"], "group-template.yaml", { type: "text/yaml" });
    const resp = await api.createGroupFromTemplate("/tmp/demo", "Demo", "", file);

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/groups/from_template",
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
  });

  it("keeps settings blueprint preview/import endpoints wired", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { applied: true } }),
    });

    const api = await import("../../src/services/api");
    const file = new File(["title: demo"], "group-template.yaml", { type: "text/yaml" });

    await api.exportGroupTemplate("g-demo");
    await api.previewGroupTemplate("g-demo", file);
    await api.importGroupTemplateReplace("g-demo", file);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "/api/v1/groups/g-demo/template/export",
      expect.objectContaining({
        headers: expect.objectContaining({ "content-type": "application/json" }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/groups/g-demo/template/preview_upload",
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      "/api/v1/groups/g-demo/template/import_replace",
      expect.objectContaining({
        method: "POST",
        body: expect.any(FormData),
      }),
    );
  });
});

describe("api bootstrap read cache", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("reuses a recent groups response inside the bootstrap window", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: "g-demo" }] } }),
    });

    const api = await import("../../src/services/api");
    const first = await api.fetchGroups();
    const second = await api.fetchGroups();

    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("invalidates the recent groups response before group writes", async () => {
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups" && method === "GET") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: `g-${fetchMock.mock.calls.length}` }] } }),
        });
      }
      if (path === "/api/v1/groups" && method === "POST") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { group_id: "g-new" } }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    await api.fetchGroups();
    await api.createGroup("New Group");
    await api.fetchGroups();

    const groupGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(groupGets).toHaveLength(2);
  });

  it("invalidates the recent groups response when auth token changes", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: "g-demo" }] } }),
    });

    const api = await import("../../src/services/api");
    await api.fetchGroups();
    api.setAuthToken("token-b");
    await api.fetchGroups();

    const groupGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(groupGets).toHaveLength(2);
  });

  it("allows event-driven callers to bypass the recent groups response", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: "g-demo" }] } }),
    });

    const api = await import("../../src/services/api");
    await api.fetchGroups();
    api.invalidateGroupsRead();
    await api.fetchGroups();

    const groupGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(groupGets).toHaveLength(2);
  });

  it("does not let an invalidated stale groups read repopulate the recent cache", async () => {
    const staleRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups" && method === "GET") {
        const callCount = fetchMock.mock.calls.filter(
          ([calledPath, calledInit]) =>
            calledPath === "/api/v1/groups" &&
            String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
        ).length;
        if (callCount === 1) {
          return staleRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: `g-${callCount}` }] } }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const stalePromise = api.fetchGroups();
    await Promise.resolve();

    api.invalidateGroupsRead();
    staleRead.resolve({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { groups: [{ group_id: "g-stale" }] } }),
    });
    await stalePromise;

    await api.fetchGroups();
    await api.fetchGroups();

    const groupGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(groupGets).toHaveLength(2);
  });

  it("reuses a recent ping response per query variant", async () => {
    fetchMock.mockImplementation((path: string) => {
      if (path === "/api/v1/ping") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { version: "1.0.0" } }),
        });
      }
      if (path === "/api/v1/ping?include_home=1") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { version: "1.0.0", home: "/tmp/cccc" } }),
        });
      }
      return Promise.reject(new Error(`unexpected request: GET ${path}`));
    });

    const api = await import("../../src/services/api");
    await api.fetchPing();
    await api.fetchPing();
    await api.fetchPing({ includeHome: true });
    await api.fetchPing({ includeHome: true });

    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("invalidates the recent web access session response before token writes", async () => {
    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/web_access/session" && method === "GET") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: { web_access_session: { can_access_global_settings: fetchMock.mock.calls.length > 1 } },
            }),
        });
      }
      if (path === "/api/v1/access-tokens" && method === "POST") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                access_token: {
                  token_id: "tok-1",
                  user_id: "user-1",
                  is_admin: false,
                  allowed_groups: [],
                  created_at: "2026-03-19T00:00:00Z",
                },
              },
            }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    await api.fetchWebAccessSession();
    await api.createAccessToken("user-1", false, []);
    await api.fetchWebAccessSession();

    const sessionGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/web_access/session" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(sessionGets).toHaveLength(2);
  });

  it("invalidates the recent web access session response when auth token changes", async () => {
    fetchMock.mockResolvedValue({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: { web_access_session: { can_access_global_settings: true } },
        }),
    });

    const api = await import("../../src/services/api");
    await api.fetchWebAccessSession();
    api.clearAuthToken();
    await api.fetchWebAccessSession();

    const sessionGets = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/web_access/session" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(sessionGets).toHaveLength(2);
  });
});

describe("api.fetchGroupPrompts invalidation", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("clears the shared read request before updateGroupPrompt", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/prompts" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/prompts" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                preamble: { kind: "preamble", source: "builtin", filename: "AGENTS.md", content: "next-preamble" },
                help: { kind: "help", source: "builtin", filename: "AGENTS.help.md", content: "next-help" },
              },
            }),
        });
      }
      if (path === "/api/v1/groups/g-demo/prompts/help" && method === "PUT") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: { kind: "help", source: "home", filename: "AGENTS.help.md", content: "updated-help" },
            }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchGroupPrompts("g-demo");
    await Promise.resolve();

    await api.updateGroupPrompt("g-demo", "help", "updated-help");
    await api.fetchGroupPrompts("g-demo");

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/prompts" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            preamble: { kind: "preamble", source: "builtin", filename: "AGENTS.md", content: "stale-preamble" },
            help: { kind: "help", source: "builtin", filename: "AGENTS.help.md", content: "stale-help" },
          },
        }),
    });
    await staleRead;
  });

  it("clears the shared read request before resetGroupPrompt", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/prompts" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/prompts" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                preamble: {
                  kind: "preamble",
                  source: "builtin",
                  filename: "AGENTS.md",
                  content: "builtin-preamble",
                },
                help: { kind: "help", source: "builtin", filename: "AGENTS.help.md", content: "builtin-help" },
              },
            }),
        });
      }
      if (path === "/api/v1/groups/g-demo/prompts/help?confirm=help" && method === "DELETE") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: { kind: "help", source: "builtin", filename: "AGENTS.help.md", content: "builtin-help" },
            }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchGroupPrompts("g-demo");
    await Promise.resolve();

    await api.resetGroupPrompt("g-demo", "help");
    await api.fetchGroupPrompts("g-demo");

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/prompts" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            preamble: { kind: "preamble", source: "home", filename: "AGENTS.md", content: "stale-preamble" },
            help: { kind: "help", source: "home", filename: "AGENTS.help.md", content: "stale-help" },
          },
        }),
    });
    await staleRead;
  });
});

describe("api.fetchContext dedupe", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("reuses the in-flight context request for the same group", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        return firstRead.promise;
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const req1 = api.fetchContext("g-demo");
    const req2 = api.fetchContext("g-demo");

    expect(fetchMock).toHaveBeenCalledTimes(1);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: {},
          },
        }),
    });

    const [resp1, resp2] = await Promise.all([req1, req2]);
    expect(resp1.ok).toBe(true);
    expect(resp2.ok).toBe(true);
  });

  it("reuses the in-flight pet peer context request for the same group", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/pet-context" && method === "GET") {
        return firstRead.promise;
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const req1 = api.fetchPetPeerContext("g-demo");
    const req2 = api.fetchPetPeerContext("g-demo");

    expect(fetchMock).toHaveBeenCalledTimes(1);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            persona: "quiet",
            snapshot: "snapshot",
            source: "help",
          },
        }),
    });

    const [resp1, resp2] = await Promise.all([req1, req2]);
    expect(resp1.ok).toBe(true);
    expect(resp2.ok).toBe(true);
  });

  it("clears the shared context request before contextSync", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/context" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                coordination: { tasks: [] },
                agent_states: [],
                attention: null,
                board: null,
                tasks_summary: { total: 1, planned: 1, active: 0, done: 0, archived: 0 },
                meta: {},
              },
            }),
        });
      }
      if (path === "/api/v1/groups/g-demo/context" && method === "POST") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { version: "next" } }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchContext("g-demo");
    await Promise.resolve();

    await api.contextSync("g-demo", [{ op: "coordination.brief.update", current_focus: "updated" }]);
    await api.fetchContext("g-demo");

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/context" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: {},
          },
        }),
    });
    await staleRead;
  });

  it("bypasses shared context inflight when fresh is requested", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        return firstRead.promise;
      }
      if (typeof path === "string" && path.startsWith("/api/v1/groups/g-demo/context?fresh=1") && method === "GET") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                coordination: { tasks: [] },
                agent_states: [],
                attention: null,
                board: null,
                tasks_summary: { total: 1, planned: 1, active: 0, done: 0, archived: 0 },
                meta: { version: "fresh" },
              },
            }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchContext("g-demo");
    await Promise.resolve();

    const freshRead = await api.fetchContext("g-demo", { fresh: true });
    expect(freshRead.ok).toBe(true);
    expect(
      fetchMock.mock.calls.filter(([path]) => String(path).startsWith("/api/v1/groups/g-demo/context")),
    ).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: { version: "stale" },
          },
        }),
    });
    await staleRead;
  });

  it("invalidates the shared context read for external write notifications", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/context" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () =>
            JSON.stringify({
              ok: true,
              result: {
                coordination: { tasks: [] },
                agent_states: [],
                attention: null,
                board: null,
                tasks_summary: { total: 1, planned: 1, active: 0, done: 0, archived: 0 },
                meta: { version: "fresh-after-external-write" },
              },
            }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchContext("g-demo");
    await Promise.resolve();

    api.invalidateContextRead("g-demo");
    const freshRead = await api.fetchContext("g-demo");
    expect(freshRead.ok).toBe(true);
    if (freshRead.ok) {
      expect(freshRead.result.meta?.version).toBe("fresh-after-external-write");
    }

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/context" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: { version: "stale" },
          },
        }),
    });
    await staleRead;
  });

  it("reuses an in-flight fresh context request for follow-up reads", async () => {
    const staleRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();
    const freshRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        return staleRead.promise;
      }
      if (typeof path === "string" && path.startsWith("/api/v1/groups/g-demo/context?fresh=1") && method === "GET") {
        return freshRead.promise;
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const stalePromise = api.fetchContext("g-demo");
    await Promise.resolve();

    const freshPromise = api.fetchContext("g-demo", { fresh: true });
    await Promise.resolve();
    const followerPromise = api.fetchContext("g-demo");

    expect(
      fetchMock.mock.calls.filter(([path]) => String(path).startsWith("/api/v1/groups/g-demo/context")),
    ).toHaveLength(2);

    freshRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 1, planned: 1, active: 0, done: 0, archived: 0 },
            meta: { version: "fresh-shared" },
          },
        }),
    });

    const [freshResp, followerResp] = await Promise.all([freshPromise, followerPromise]);
    expect(freshResp.ok).toBe(true);
    expect(followerResp.ok).toBe(true);
    if (followerResp.ok) {
      expect(followerResp.result.meta?.version).toBe("fresh-shared");
    }

    staleRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: { version: "stale" },
          },
        }),
    });
    await stalePromise;
  });

  it("does not let an older request finally clear a newer shared mapping", async () => {
    const staleRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();
    const freshRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();
    const postFreshRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/context" && method === "GET") {
        const samePathCalls = fetchMock.mock.calls.filter(
          ([calledPath, calledInit]) =>
            calledPath === "/api/v1/groups/g-demo/context" &&
            String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
        ).length;
        if (samePathCalls === 1) return staleRead.promise;
        if (samePathCalls === 2) return postFreshRead.promise;
      }
      if (typeof path === "string" && path.startsWith("/api/v1/groups/g-demo/context?fresh=1") && method === "GET") {
        return freshRead.promise;
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const stalePromise = api.fetchContext("g-demo");
    await Promise.resolve();

    const freshPromise = api.fetchContext("g-demo", { fresh: true });
    await Promise.resolve();

    staleRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
            meta: { version: "stale" },
          },
        }),
    });
    await stalePromise;

    const afterStaleWhileFreshInFlight = api.fetchContext("g-demo");
    expect(
      fetchMock.mock.calls.filter(([path]) => String(path).startsWith("/api/v1/groups/g-demo/context")),
    ).toHaveLength(2);

    freshRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 1, planned: 1, active: 0, done: 0, archived: 0 },
            meta: { version: "fresh" },
          },
        }),
    });

    const [freshResp, followerResp] = await Promise.all([freshPromise, afterStaleWhileFreshInFlight]);
    expect(freshResp.ok).toBe(true);
    expect(followerResp.ok).toBe(true);
    if (followerResp.ok) {
      expect(followerResp.result.meta?.version).toBe("fresh");
    }

    const postFreshPromise = api.fetchContext("g-demo");
    expect(
      fetchMock.mock.calls.filter(([path]) => String(path).startsWith("/api/v1/groups/g-demo/context")),
    ).toHaveLength(3);

    postFreshRead.resolve({
      status: 200,
      ok: true,
      text: async () =>
        JSON.stringify({
          ok: true,
          result: {
            coordination: { tasks: [] },
            agent_states: [],
            attention: null,
            board: null,
            tasks_summary: { total: 2, planned: 2, active: 0, done: 0, archived: 0 },
            meta: { version: "post-fresh" },
          },
        }),
    });

    const postFreshResp = await postFreshPromise;
    expect(postFreshResp.ok).toBe(true);
    if (postFreshResp.ok) {
      expect(postFreshResp.result.meta?.version).toBe("post-fresh");
    }
  });
});

describe("api.fetchActors invalidation", () => {
  beforeEach(() => {
    vi.resetModules();
    fetchMock.mockReset();
    sessionStorageMock.clear();
  });

  afterEach(async () => {
    const api = await import("../../src/services/api");
    api.clearAuthToken();
  });

  it("clears the shared pure-read request before actor writes", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/actors" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/actors" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { actors: [{ id: "peer-new" }] } }),
        });
      }
      if (path === "/api/v1/groups/g-demo/actors/peer-1" && method === "POST") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { actor: { id: "peer-1" } } }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchActors("g-demo", false);
    await Promise.resolve();

    await api.updateActor("g-demo", "peer-1", "codex");
    await api.fetchActors("g-demo", false);

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/actors" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { actors: [{ id: "peer-old" }] } }),
    });
    await staleRead;
  });

  it("clears the shared pure-read request before group lifecycle writes", async () => {
    const firstRead = createDeferred<{
      status: number;
      ok: boolean;
      text: () => Promise<string>;
    }>();

    fetchMock.mockImplementation((path: string, init?: RequestInit) => {
      const method = String(init?.method || "GET").toUpperCase();
      if (path === "/api/v1/groups/g-demo/actors" && method === "GET") {
        if (
          fetchMock.mock.calls.filter(
            ([calledPath, calledInit]) =>
              calledPath === "/api/v1/groups/g-demo/actors" &&
              String((calledInit as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
          ).length === 1
        ) {
          return firstRead.promise;
        }
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: { actors: [{ id: "peer-started" }] } }),
        });
      }
      if (path === "/api/v1/groups/g-demo/start?by=user" && method === "POST") {
        return Promise.resolve({
          status: 200,
          ok: true,
          text: async () => JSON.stringify({ ok: true, result: {} }),
        });
      }
      return Promise.reject(new Error(`unexpected request: ${method} ${path}`));
    });

    const api = await import("../../src/services/api");
    const staleRead = api.fetchActors("g-demo", false);
    await Promise.resolve();

    await api.startGroup("g-demo");
    await api.fetchActors("g-demo", false);

    const getCalls = fetchMock.mock.calls.filter(
      ([path, init]) =>
        path === "/api/v1/groups/g-demo/actors" &&
        String((init as RequestInit | undefined)?.method || "GET").toUpperCase() === "GET",
    );
    expect(getCalls).toHaveLength(2);

    firstRead.resolve({
      status: 200,
      ok: true,
      text: async () => JSON.stringify({ ok: true, result: { actors: [{ id: "peer-stale" }] } }),
    });
    await staleRead;
  });
});
