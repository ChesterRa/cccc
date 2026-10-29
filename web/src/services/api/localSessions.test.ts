// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { setAuthToken } from "./base";
import { fetchActors } from "./actors";
import { fetchGroups } from "./groups";
import {
  createLocalConversation,
  fetchLocalSessionRecovery,
  fetchLocalSessions,
  joinLocalSessions,
  refreshLocalSessionRecovery,
  resumeLocalSessionAccess,
} from "./localSessions";
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  setAuthToken("");
});
it("invalidates the existing Groups read when a native conversation is created", async () => {
  vi.useFakeTimers();
  setAuthToken("");
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, result: { groups: [] } })))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: true,
          result: { group_id: "g_native", group: { group_id: "g_native" }, reused: false },
        }),
      ),
    )
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ ok: true, result: { groups: [{ group_id: "g_native" }] } })),
    );
  vi.stubGlobal("fetch", fetch);
  await fetchGroups();
  await createLocalConversation(["codex:fixture"]);
  expect(await fetchGroups()).toEqual({ ok: true, result: { groups: [{ group_id: "g_native" }] } });
  expect(fetch).toHaveBeenCalledTimes(3);
});

it("reads the sources directory without changing its public response shape or starting a conversation", async () => {
  const source = {
    source_id: "codex:fixture",
    runtime: "codex",
    native_session_id: "fixture",
    title: "Native fixture",
    cwd: "/synthetic/project",
    connected: true,
  };
  const fetch = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: { sources: [source] } })));
  vi.stubGlobal("fetch", fetch);
  expect(await fetchLocalSessions()).toEqual({ ok: true, result: { sources: [source] } });
  expect(fetch).toHaveBeenCalledOnce();
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/local_sessions");
  expect(fetch.mock.calls[0][1].method ?? "GET").toBe("GET");
});

it.each([
  {
    source_id: "codex:native-thread",
    recovery: {
      state: "host_exited",
      cwd: "/synthetic/original workspace",
      shell: "posix",
      command:
        "cd -- '/synthetic/original workspace' && CCCC_HOME='/synthetic/cccc' CODEX_HOME='/synthetic/codex' codex resume 'native-thread'",
    },
  },
  {
    source_id: "claude:host-not-conversation",
    recovery: {
      state: "unknown",
      cwd: null,
      shell: "posix",
      command: "CCCC_HOME='/synthetic/cccc' claude --resume",
    },
  },
])("reads Actor-bound recovery with both path segments encoded for $source_id", async (result) => {
  const groupId = "g/with space?#%";
  const actorId = "actor/日本:one?#%";
  const expected = { ok: true, result: { group_id: groupId, actor_id: actorId, ...result } };
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(expected)));
  vi.stubGlobal("fetch", fetch);

  expect(await fetchLocalSessionRecovery(groupId, actorId)).toEqual(expected);
  expect(fetch).toHaveBeenCalledOnce();
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/local_sessions/recovery/g%2Fwith%20space%3F%23%25/actor%2F%E6%97%A5%E6%9C%AC%3Aone%3F%23%25",
  );
  expect(fetch.mock.calls[0][1].method ?? "GET").toBe("GET");
  expect(fetch.mock.calls[0][1].body).toBeUndefined();
});

it("preserves a failed recovery read without replacing it with an exit state", async () => {
  const expected = {
    ok: false,
    error: { code: "synthetic_read_failure", message: "Recovery metadata could not be read" },
  };
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(expected), { status: 500 }));
  vi.stubGlobal("fetch", fetch);
  expect(await fetchLocalSessionRecovery("g_original", "native")).toEqual(expected);
  expect(fetch).toHaveBeenCalledOnce();
});

it("explicitly refreshes the Actor-bound channel with one encoded POST and the same recovery response", async () => {
  const expected = {
    ok: true,
    result: {
      group_id: "g/one ?#",
      actor_id: "native/日本",
      source_id: "codex:thread",
      refresh_status: "checked",
      recovery: {
        state: "connected",
        cwd: "C:\\synthetic\\project",
        shell: "powershell",
        command: "Set-Location -LiteralPath 'C:\\synthetic\\project'; codex resume 'thread'",
      },
    },
  };
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(expected)));
  vi.stubGlobal("fetch", fetch);
  expect(
    await refreshLocalSessionRecovery(expected.result.group_id, expected.result.actor_id),
  ).toEqual(expected);
  expect(fetch).toHaveBeenCalledExactlyOnceWith(
    "/api/v1/local_sessions/recovery/g%2Fone%20%3F%23/native%2F%E6%97%A5%E6%9C%AC/refresh",
    expect.objectContaining({ method: "POST" }),
  );
  expect(fetch.mock.calls[0][1].body).toBeUndefined();
});

it("does not retry an interrupted eligibility refresh or hide its failure", async () => {
  vi.useFakeTimers();
  const fetch = vi.fn().mockRejectedValue(new Error("Synthetic interrupted refresh"));
  vi.stubGlobal("fetch", fetch);
  expect(await refreshLocalSessionRecovery("g_one", "native")).toEqual({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Synthetic interrupted refresh" },
  });
  await vi.advanceTimersByTimeAsync(60000);
  expect(fetch).toHaveBeenCalledOnce();
});

it.each([
  { sourceIds: ["codex:fixture"], direct: false, reused: false },
  { sourceIds: ["claude:first", "codex:second"], direct: false, reused: false },
])(
  "explicitly creates a new Group with direct=$direct for $sourceIds",
  async ({ sourceIds, direct, reused }) => {
    const group = { group_id: "g_native", state: "active" };
    const fetch = vi
      .fn()
      .mockResolvedValue(
        new Response(
          JSON.stringify({ ok: true, result: { group_id: group.group_id, group, reused } }),
        ),
      );
    vi.stubGlobal("fetch", fetch);
    const response = await createLocalConversation(sourceIds);
    expect(fetch).toHaveBeenCalledExactlyOnceWith(
      "/api/v1/local_sessions/conversations",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ source_ids: sourceIds, direct }),
      }),
    );
    expect(response.ok).toBe(true);
    if (!response.ok) throw new Error(response.error.message);
    expect(response.result.group_id).toBe(group.group_id);
    expect(response.result.group).toEqual(group);
    expect(response.result.reused).toBe(reused);
  },
);

it.each([false, true])(
  "joins the explicit Group in selection order with reused=%s",
  async (reused) => {
    const sourceIds = ["claude:first", "codex:second"];
    const group = { group_id: "g_existing", state: "paused", active_scope_key: "original-scope" };
    const expected = { ok: true, result: { group_id: group.group_id, group, reused } };
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(expected)));
    vi.stubGlobal("fetch", fetch);

    expect(await joinLocalSessions(group.group_id, sourceIds)).toEqual(expected);
    expect(fetch).toHaveBeenCalledExactlyOnceWith(
      "/api/v1/local_sessions/join",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ group_id: group.group_id, source_ids: sourceIds }),
      }),
    );
    expect(sourceIds).toEqual(["claude:first", "codex:second"]);
  },
);

it.each([false, true])(
  "invalidates Groups and actors reads started during a join (includeInternal=%s)",
  async (includeInternal) => {
    vi.useFakeTimers();
    const group = { group_id: "g_join_cache", title: "Updated Group" };
    const actors = [{ id: "existing" }, { id: "new-native" }];
    let finishJoin!: (response: Response) => void;
    let finishOldActors!: (response: Response) => void;
    const fetch = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<Response>((resolve) => {
            finishJoin = resolve;
          }),
      )
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, result: { groups: [] } })))
      .mockImplementationOnce(
        () =>
          new Promise<Response>((resolve) => {
            finishOldActors = resolve;
          }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ok: true, result: { groups: [group] } })),
      )
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, result: { actors } })));
    vi.stubGlobal("fetch", fetch);

    const joining = joinLocalSessions(group.group_id, ["codex:new-native"]);
    await fetchGroups();
    const oldActors = fetchActors(group.group_id, false, undefined, { includeInternal });
    finishJoin(
      new Response(
        JSON.stringify({ ok: true, result: { group_id: group.group_id, group, reused: false } }),
      ),
    );
    await joining;
    expect(await fetchGroups()).toEqual({ ok: true, result: { groups: [group] } });
    const freshActors = fetchActors(group.group_id, false, undefined, { includeInternal });
    finishOldActors(
      new Response(JSON.stringify({ ok: true, result: { actors: [{ id: "existing" }] } })),
    );
    expect(await freshActors).toEqual({ ok: true, result: { actors } });
    await oldActors;
    expect(fetch).toHaveBeenCalledTimes(5);
  },
);

it("returns an interrupted join as an error without retrying the mutation", async () => {
  vi.useFakeTimers();
  const fetch = vi.fn().mockRejectedValue(new Error("Synthetic interrupted join"));
  vi.stubGlobal("fetch", fetch);
  expect(await joinLocalSessions("g_existing", ["codex:fixture"])).toEqual({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Synthetic interrupted join" },
  });
  await vi.advanceTimersByTimeAsync(9000);
  expect(fetch).toHaveBeenCalledOnce();
});

it("resumes only the selected source through the existing Actor update route and invalidates read caches", async () => {
  vi.useFakeTimers();
  const groupId = "g/native";
  const actorId = "actor/日本";
  const fetch = vi.fn().mockImplementation(async (path: string, options: RequestInit) => {
    const actors = [
      { id: actorId, enabled: options.method === "POST" || fetch.mock.calls.length > 3 },
    ];
    return new Response(
      JSON.stringify({
        ok: true,
        result:
          path === "/api/v1/groups"
            ? { groups: [] }
            : options.method === "POST"
              ? { actor: actors[0] }
              : { actors },
      }),
    );
  });
  vi.stubGlobal("fetch", fetch);
  await fetchActors(groupId, false);
  await fetchGroups();
  expect((await resumeLocalSessionAccess(groupId, actorId, "codex:chosen")).ok).toBe(true);
  expect(fetch.mock.calls[2]).toEqual([
    "/api/v1/groups/g%2Fnative/actors/actor%2F%E6%97%A5%E6%9C%AC",
    expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ by: "user", enabled: true, external_session_id: "codex:chosen" }),
    }),
  ]);
  await fetchActors(groupId, false);
  await fetchGroups();
  expect(fetch).toHaveBeenCalledTimes(5);
});
