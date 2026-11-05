// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import {
  completeActorRuntimeSwitch,
  switchActorRuntime,
  type ActorRuntimeSwitchRequest,
} from "./actors";
import { setAuthToken } from "./base";

const request: ActorRuntimeSwitchRequest = {
  expected_actor_generation: "generation-1",
  expected_actor_updated_at: "2026-01-01T00:00:00Z",
  expected_source_id: "codex:old",
  target: { kind: "native", source_id: "claude:new", expected_native_session_id: "session-1" },
};

afterEach(() => {
  vi.unstubAllGlobals();
  setAuthToken("");
});

it("forwards the confirmed target and accepts a result for the requested member", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g/one",
            actor_id: "actor/one",
            op_id: "operation-1",
            switched: true,
            from: { kind: "native", runtime: "codex" },
            to: { kind: "native", runtime: "claude" },
            completion: "done",
            start: "native",
          },
        }),
      ),
    );
  vi.stubGlobal("fetch", fetch);
  const response = await switchActorRuntime("g/one", "actor/one", request);
  expect(response.ok).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/groups/g%2Fone/actors/actor%2Fone/runtime_switch");
  expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual(request);
});

it("reports an unknown result for an interrupted, empty, or mismatched response", async () => {
  const fetch = vi
    .fn()
    .mockRejectedValueOnce(new Error("connection lost"))
    .mockResolvedValueOnce(new Response("", { status: 200 }))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g",
            actor_id: "other",
            op_id: "operation-1",
            switched: true,
            from: { kind: "native", runtime: "codex" },
            to: { kind: "native", runtime: "claude" },
            completion: "done",
            start: "native",
          },
        }),
      ),
    );
  vi.stubGlobal("fetch", fetch);
  for (let index = 0; index < 3; index += 1) {
    const response = await switchActorRuntime("g", "actor", request);
    expect(response.ok).toBe(false);
    if (!response.ok) expect(response.error.code).toBe("actor_switch_unknown");
  }
  expect(fetch).toHaveBeenCalledTimes(3);
});

it("retains definite daemon errors and validates completion responses", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ ok: false, error: { code: "actor_switch_conflict", message: "changed" } }),
        { status: 409 },
      ),
    )
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: true,
          result: { group_id: "g", actor_id: "actor", switched: false, completion: "done" },
        }),
      ),
    )
    .mockResolvedValueOnce(new Response("", { status: 200 }));
  vi.stubGlobal("fetch", fetch);
  const conflict = await switchActorRuntime("g", "actor", request);
  expect(conflict.ok).toBe(false);
  if (!conflict.ok) expect(conflict.error.code).toBe("actor_switch_conflict");
  expect(await completeActorRuntimeSwitch("g", "actor")).toMatchObject({
    ok: true,
    result: { group_id: "g", actor_id: "actor", switched: false, completion: "done" },
  });
  expect(await completeActorRuntimeSwitch("g", "actor")).toMatchObject({
    ok: false,
    error: { code: "actor_switch_unknown" },
  });
});

it("treats a server failure as unknown but preserves a definite incomplete result", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ ok: false, error: { code: "proxy_error", message: "lost" } }), {
        status: 502,
      }),
    )
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ ok: false, error: { code: "timeout", message: "lost" } }), {
        status: 408,
      }),
    )
    .mockResolvedValueOnce(new Response("unrecognized proxy response", { status: 400 }))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: false,
          error: {
            code: "actor_switch_incomplete",
            message: "old owner could not be stopped",
            details: { old_owner: "unknown" },
          },
        }),
        { status: 400 },
      ),
    );
  vi.stubGlobal("fetch", fetch);
  expect(await switchActorRuntime("g", "actor", request)).toMatchObject({
    ok: false,
    error: { code: "actor_switch_unknown" },
  });
  expect(await switchActorRuntime("g", "actor", request)).toMatchObject({
    ok: false,
    error: { code: "actor_switch_unknown" },
  });
  expect(await switchActorRuntime("g", "actor", request)).toMatchObject({
    ok: false,
    error: { code: "actor_switch_unknown" },
  });
  expect(await switchActorRuntime("g", "actor", request)).toMatchObject({
    ok: false,
    error: { code: "actor_switch_incomplete", details: { old_owner: "unknown" } },
  });
});

it("recognizes an already committed switch held for completion", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              group_id: "g",
              actor_id: "actor",
              op_id: "operation-2",
              switched: true,
              from: { kind: "native", runtime: "codex" },
              to: { kind: "managed", runtime: "claude" },
              completion: "pending",
              start: "held",
            },
          }),
        ),
      ),
  );
  expect(await switchActorRuntime("g", "actor", request)).toMatchObject({
    ok: true,
    result: { completion: "pending", start: "held" },
  });
});
