import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  cancelReplyRequest,
  contextSync,
  deleteCoordinationTaskConfirmed,
  deliverMessage,
  enableGroupCapability,
  fetchContext,
  fetchSlashCommandCapabilityState,
  fetchGroupCapabilityState,
  moveCoordinationTask,
  restoreCoordinationTask,
  uninstallCapability,
  updateGroupCapabilityVisibility,
  updateCoordinationTask,
  replyMessage,
  sendCrossGroupMessage,
  sendMessage,
} from "./context";

describe("task revision guards", () => {
  const taskReceipt = (...ops: string[]) =>
    new Response(
      JSON.stringify({
        ok: true,
        result: { success: true, dry_run: false, changes: ops.map((op, index) => ({ index, op })) },
      }),
    );

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("fences the edit before the move in the same batch and passes conflicts through", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const conflict = {
      ok: false,
      error: { code: "task_conflict", message: "changed", details: { expected: 0, actual: 1 } },
    };
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify(conflict), {
          status: 409,
          headers: { "content-type": "application/json" },
        }),
      );

    const response = await updateCoordinationTask(
      "g1",
      "T001",
      { title: "new title", status: "active", notes: "old draft" },
      0,
    );

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body))).toMatchObject({
      by: "user",
      ops: [
        {
          op: "task.update",
          task_id: "T001",
          if_task_rev: 0,
          title: "new title",
          notes: "old draft",
        },
        { op: "task.move", task_id: "T001", status: "active" },
      ],
    });
    expect(JSON.parse(String(request.body)).ops[1]).not.toHaveProperty("if_task_rev");
    expect(response).toEqual(conflict);
  });

  it("sends only the fields explicitly changed, and only a guarded move for status alone", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(taskReceipt("task.update"))
      .mockResolvedValueOnce(taskReceipt("task.move"));

    expect(
      await updateCoordinationTask("g1", "T001", { title: "new title", priority: null }, 3),
    ).toMatchObject({ ok: true });
    expect(await updateCoordinationTask("g1", "T001", { status: "done" }, 4)).toMatchObject({
      ok: true,
    });

    const bodies = fetchMock.mock.calls.map((call) => JSON.parse(String(call[1]?.body)));
    expect(bodies[0].ops).toEqual([
      { op: "task.update", task_id: "T001", if_task_rev: 3, title: "new title", priority: "" },
    ]);
    expect(bodies[1].ops).toEqual([
      { op: "task.move", task_id: "T001", status: "done", if_task_rev: 4 },
    ]);
  });

  it("treats an empty patch as a local no-op", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch");

    expect(await updateCoordinationTask("g1", "T001", {}, undefined)).toMatchObject({ ok: true });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not treat an empty HTTP success as confirmation of a task write", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("", { status: 200 }));

    expect(await updateCoordinationTask("g1", "T001", { title: "draft" }, 1)).toMatchObject({
      ok: false,
      error: { code: "task_receipt_unconfirmed" },
    });
  });

  it("requires each task operation and the created task ID in a success receipt", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const response = (changes: unknown, dryRun = false) =>
      new Response(
        JSON.stringify({ ok: true, result: { success: true, dry_run: dryRun, changes } }),
      );
    fetchMock
      .mockResolvedValueOnce(response([{ index: 0, op: "task.create" }]))
      .mockResolvedValueOnce(response([{ index: 0, op: "task.create", task_id: "T002" }]))
      .mockResolvedValueOnce(response([{ index: 0, op: "task.move" }]))
      .mockResolvedValueOnce(response([{ index: 0, op: "task.update" }], true));

    const create = [{ op: "task.create", title: "Task" }];
    const update = [{ op: "task.update", task_id: "T001", title: "Task" }];
    expect(await contextSync("g1", create)).toMatchObject({
      ok: false,
      error: { code: "task_receipt_unconfirmed" },
    });
    expect(await contextSync("g1", create)).toMatchObject({ ok: true });
    expect(await contextSync("g1", update)).toMatchObject({
      ok: false,
      error: { code: "task_receipt_unconfirmed" },
    });
    expect(await contextSync("g1", update)).toMatchObject({
      ok: false,
      error: { code: "task_receipt_unconfirmed" },
    });
  });

  it("checks every operation in a task batch and accepts a matching dry run", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const ops = [
      { op: "task.update", task_id: "T001", title: "Task" },
      { op: "task.move", task_id: "T001", status: "active" },
    ];
    const result = (changes: unknown, dryRun: boolean) =>
      new Response(
        JSON.stringify({ ok: true, result: { success: true, dry_run: dryRun, changes } }),
      );
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(result([{ index: 0, op: "task.update" }], false))
      .mockResolvedValueOnce(
        result(
          [
            { index: 0, op: "task.update" },
            { index: 1, op: "task.move" },
          ],
          true,
        ),
      );

    expect(await contextSync("g1", ops)).toMatchObject({
      ok: false,
      error: { code: "task_receipt_unconfirmed" },
    });
    expect(await contextSync("g1", ops, true)).toMatchObject({ ok: true });
  });

  it("includes the observed revision in an independent move", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(taskReceipt("task.move"));

    expect(await moveCoordinationTask("g1", "T001", "done", 7)).toMatchObject({ ok: true });

    const bodies = fetchMock.mock.calls.map((call) => JSON.parse(String(call[1]?.body)));
    expect(bodies[0].ops).toEqual([
      { op: "task.move", task_id: "T001", status: "done", if_task_rev: 7 },
    ]);
  });

  it("restores an archived task with its observed revision", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(taskReceipt("task.restore"));

    expect(await restoreCoordinationTask("g1", "T001", 7)).toMatchObject({ ok: true });

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body)).ops).toEqual([
      { op: "task.restore", task_id: "T001", if_task_rev: 7 },
    ]);
    expect(await restoreCoordinationTask("g1", "T001", undefined)).toMatchObject({
      ok: false,
      error: { code: "task_revision_unavailable" },
    });
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  const detailResponse = (tasksVersion = "tasksv:9", rev = 0, total = 2) =>
    new Response(
      JSON.stringify({
        ok: true,
        result: {
          task: { id: "T002", rev },
          tasks_version: tasksVersion,
          delete_info: { allowed: true, total },
        },
      }),
    );
  const contextResponse = (tasksVersion = "tasksv:9") =>
    new Response(
      JSON.stringify({
        ok: true,
        result: { version: "ctxv:4", tasks_version: tasksVersion, coordination: { tasks: [] } },
      }),
    );
  const confirmed = { rev: 0, total: 2, tasksVersion: "tasksv:9" };

  it("checks the confirmed task snapshot and sends both revision guards for deletion", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(detailResponse())
      .mockResolvedValueOnce(contextResponse())
      .mockResolvedValueOnce(taskReceipt("task.delete"));

    const result = await deleteCoordinationTaskConfirmed("g1", "T002", confirmed);

    expect(result.ok).toBe(true);
    expect(fetchMock.mock.calls[0]?.[1]?.cache).toBe("no-store");
    expect(fetchMock.mock.calls[1]?.[1]?.cache).toBe("no-store");
    const request = fetchMock.mock.calls[2]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body))).toMatchObject({
      if_version: "ctxv:4",
      ops: [{ op: "task.delete", task_id: "T002", if_task_rev: 0 }],
    });
  });

  it("rejects a replaced child even if the root revision and total are unchanged", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(detailResponse("tasksv:10"));

    const result = await deleteCoordinationTaskConfirmed("g1", "T002", confirmed);

    expect(result).toMatchObject({ ok: false, error: { code: "task_changed" } });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("rejects a task change between the detail and context reads", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(detailResponse())
      .mockResolvedValueOnce(contextResponse("tasksv:10"));

    const result = await deleteCoordinationTaskConfirmed("g1", "T002", confirmed);

    expect(result).toMatchObject({ ok: false, error: { code: "task_changed" } });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("turns a later context conflict into a request to confirm again", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(detailResponse())
      .mockResolvedValueOnce(contextResponse())
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ok: false, error: { code: "version_conflict" } }), {
          status: 409,
        }),
      );

    expect(await deleteCoordinationTaskConfirmed("g1", "T002", confirmed)).toMatchObject({
      ok: false,
      error: { code: "task_changed" },
    });
  });

  it("leaves deletion unattempted and reports a failed context read", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(detailResponse())
      .mockRejectedValueOnce(new Error("offline"));

    const result = await deleteCoordinationTaskConfirmed("g1", "T002", confirmed);

    expect(result).toMatchObject({ ok: false, error: { code: "task_delete_check_unavailable" } });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("does not send a deletion when reading the task fails", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockRejectedValueOnce(new Error("offline"));

    expect(await deleteCoordinationTaskConfirmed("g1", "T002", confirmed)).toMatchObject({
      ok: false,
      error: { code: "task_delete_check_unavailable" },
    });
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("does not issue a delete when the task has already disappeared", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({ ok: false, error: { code: "task_not_found", message: "gone" } }),
          { status: 404 },
        ),
      );

    const result = await deleteCoordinationTaskConfirmed("g1", "T002", confirmed);

    expect(result).toMatchObject({ ok: false, error: { code: "task_not_found" } });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it.each(["permission_denied", "group_not_found"])(
    "keeps the known %s reason when the deletion check is refused",
    async (code) => {
      vi.stubGlobal("window", { location: { search: "" } });
      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ ok: false, error: { code, message: "refused" } }), {
            status: 403,
          }),
        );

      expect(await deleteCoordinationTaskConfirmed("g1", "T002", confirmed)).toMatchObject({
        ok: false,
        error: { code },
      });
      expect(fetchMock).toHaveBeenCalledOnce();
    },
  );

  it.each([undefined, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, Number.NaN])(
    "refuses all task writes when the revision %s is not trustworthy",
    async (revision) => {
      vi.stubGlobal("window", { location: { search: "" } });
      const fetchMock = vi.spyOn(globalThis, "fetch");

      const responses = await Promise.all([
        updateCoordinationTask("g1", "T001", { title: "changed" }, revision),
        moveCoordinationTask("g1", "T001", "active", revision),
        deleteCoordinationTaskConfirmed("g1", "T001", {
          rev: revision,
          total: 1,
          tasksVersion: "tasksv:1",
        }),
      ]);

      for (const response of responses) {
        expect(response).toMatchObject({ ok: false, error: { code: "task_revision_unavailable" } });
      }
      expect(fetchMock).not.toHaveBeenCalled();
    },
  );

  it.each([
    { rev: 1, total: 0, tasksVersion: "tasksv:1" },
    { rev: 1, total: 1, tasksVersion: "" },
  ])("refuses deletion without a complete confirmed snapshot", async (snapshot) => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch");

    expect(await deleteCoordinationTaskConfirmed("g1", "T001", snapshot)).toMatchObject({
      ok: false,
      error: { code: "task_revision_unavailable" },
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

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
