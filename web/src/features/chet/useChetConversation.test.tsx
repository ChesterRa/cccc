// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { ApiResponse } from "../../services/api/base";
import type { ChetGroupContext, ChetPage, ChetRecord, ChetStatus } from "../../services/api/chet";

vi.mock("../../services/api/chet", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/api/chet")>();
  return {
    ...actual,
    fetchChetHistory: vi.fn(),
    fetchChetHistoryAfter: vi.fn(),
    fetchChetInput: vi.fn(),
    fetchChetStatus: vi.fn(),
    submitChetInput: vi.fn(),
  };
});

import * as api from "../../services/api/chet";
import { CHET_DRAFT_KEY, CHET_PENDING_PREFIX, parseChetPending } from "./chetDraft";
import type { ChetRequestItem } from "./chetTimeline";
import { useChetConversation, type ChetConversation } from "./useChetConversation";
import { resendId } from "./chetResend";

const GROUP_A: ChetGroupContext = { group_id: "g-a", group_title: "Design", scope_key: "s1" };
const GROUP_B: ChetGroupContext = { group_id: "g-b", group_title: "Release", scope_key: null };

let host: HTMLDivElement;
let root: Root;
let chet: ChetConversation;

function Harness(props: {
  open: boolean;
  currentContext: ChetGroupContext | null;
  followClosed?: boolean;
}) {
  chet = useChetConversation({ enabled: true, ...props });
  return null;
}

const ok = <T,>(result: T): ApiResponse<T> => ({ ok: true, result });
const fail = (code: string): ApiResponse<never> => ({ ok: false, error: { code, message: code } });
const page = (
  records: ChetRecord[] = [],
  after = "0",
  sources: ChetPage["sources"] = new Map(),
): ApiResponse<ChetPage> =>
  ok({ records, sources, skipped: 0, before: null, after, has_more: false });
const facts = (records: ChetRecord[]) => ok({ records, sources: new Map() });
const status = (overrides: Partial<ChetStatus> = {}): ApiResponse<ChetStatus> =>
  ok({
    generation: "g1",
    preparing: [],
    queued: [],
    working: [],
    history_write_failed: false,
    owner_busy: false,
    ...overrides,
  });
const inputRecord = (
  id: string,
  text: string,
  context: ChetGroupContext | null = null,
): ChetRecord => ({
  kind: "input",
  id,
  created_at: "2026-11-01T10:00:00.000000Z",
  request: { text, context },
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function render(
  currentContext: ChetGroupContext | null = GROUP_A,
  open = true,
  followClosed = false,
) {
  await act(async () =>
    root.render(
      <Harness open={open} currentContext={currentContext} followClosed={followClosed} />,
    ),
  );
  await act(async () => vi.advanceTimersByTimeAsync(0));
}

const flush = () => act(async () => vi.advanceTimersByTimeAsync(0));
const requests = () =>
  chet.items.filter((item): item is ChetRequestItem => item.kind === "request");
const pendingKeys = () =>
  Object.keys(window.localStorage).filter((key) => key.startsWith(CHET_PENDING_PREFIX));

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  window.localStorage.clear();
  vi.mocked(api.fetchChetStatus).mockResolvedValue(status());
  vi.mocked(api.fetchChetHistory).mockResolvedValue(page());
  vi.mocked(api.fetchChetHistoryAfter).mockResolvedValue(page());
  vi.mocked(api.fetchChetInput).mockResolvedValue(facts([]));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.resetAllMocks();
  vi.useRealTimers();
});

it("stores the request ID before posting and lets go of it once history shows it", async () => {
  vi.mocked(api.submitChetInput).mockImplementation(async (id) => {
    expect(window.localStorage.getItem(CHET_PENDING_PREFIX + id)).not.toBeNull();
    return ok("inserted");
  });
  await render();
  await act(async () => chet.setText("What is waiting for me?"));
  await act(async () => chet.send());
  const [id, request] = vi.mocked(api.submitChetInput).mock.calls[0]!;
  expect(request).toEqual({ text: "What is waiting for me?", context: GROUP_A });
  expect(chet.draft).toBe("");
  expect(requests()).toMatchObject([{ id, state: "preparing" }]);

  vi.mocked(api.fetchChetHistoryAfter).mockResolvedValueOnce(
    page([inputRecord(id, "What is waiting for me?", GROUP_A)], "1"),
  );
  await act(async () => vi.advanceTimersByTimeAsync(1_500));
  expect(pendingKeys()).toEqual([]);
  expect(requests()).toHaveLength(1);
});

it("lets go of a send that history showed before its response came back", async () => {
  await render();
  const response = deferred<ApiResponse<"inserted">>();
  vi.mocked(api.submitChetInput).mockReturnValueOnce(response.promise);
  await act(async () => chet.setText("quick"));
  await act(async () => chet.send());
  const id = vi.mocked(api.submitChetInput).mock.calls[0]![0];
  vi.mocked(api.fetchChetHistoryAfter).mockResolvedValueOnce(
    page([inputRecord(id, "quick", GROUP_A)], "1"),
  );
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(pendingKeys()).toEqual([]);
  await act(async () => response.resolve(ok("inserted")));
  expect(pendingKeys()).toEqual([]);
  expect(requests()).toHaveLength(1);
});

it("sends from a page without crypto.randomUUID, keeping the draft until an ID exists", async () => {
  vi.stubGlobal("crypto", { getRandomValues: crypto.getRandomValues.bind(crypto) });
  try {
    vi.mocked(api.submitChetInput).mockResolvedValueOnce(ok("inserted"));
    await render();
    await act(async () => chet.setText("over plain HTTP"));
    await act(async () => chet.send());
    const [id] = vi.mocked(api.submitChetInput).mock.calls[0]!;
    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(chet.draft).toBe("");
  } finally {
    vi.unstubAllGlobals();
  }
});

it("keeps a send with a lost response and retries it with the same ID", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(fail("NETWORK_ERROR"));
  await render();
  await act(async () => chet.setText("hand this over"));
  await act(async () => chet.send());
  const id = vi.mocked(api.submitChetInput).mock.calls[0]![0];
  expect(requests()).toMatchObject([{ id, state: "send_unknown" }]);
  expect(pendingKeys()).toEqual([CHET_PENDING_PREFIX + id]);

  vi.mocked(api.submitChetInput).mockResolvedValueOnce(ok("existing"));
  await act(async () => chet.retry(id));
  expect(vi.mocked(api.submitChetInput).mock.calls[1]).toEqual([
    id,
    { text: "hand this over", context: GROUP_A },
  ]);
  expect(requests()[0].state).toBe("preparing");
});

it("gives a refused first send back to the composer and holds nothing", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce({
    ok: false,
    error: {
      code: "codex_voice_recovery_pending",
      message: "pending",
      details: { recovery_owners: ["web"] },
    },
  });
  await render();
  await act(async () => chet.setText("start please"));
  await act(async () => chet.send());
  expect(chet.draft).toBe("start please");
  expect(chet.composerProblem).toEqual({
    code: "codex_voice_recovery_pending",
    message: "pending",
    recoveryOwners: ["web"],
  });
  expect(requests()).toEqual([]);
  expect(pendingKeys()).toEqual([]);
});

it("keeps an unknown send when its retry is refused", async () => {
  vi.mocked(api.submitChetInput)
    .mockResolvedValueOnce(fail("chet_history_unknown"))
    .mockResolvedValueOnce(fail("permission_denied"));
  await render();
  await act(async () => chet.setText("maybe sent"));
  await act(async () => chet.send());
  const id = vi.mocked(api.submitChetInput).mock.calls[0]![0];
  await act(async () => chet.retry(id));
  expect(requests()).toMatchObject([{ id, state: "send_unknown" }]);
  expect(chet.problems.get(id)).toMatchObject({ kind: "retry_refused", code: "permission_denied" });
  expect(pendingKeys()).toEqual([CHET_PENDING_PREFIX + id]);
});

it("marks an ID conflict and offers no retry with that ID", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(fail("chet_history_id_conflict"));
  await render();
  await act(async () => chet.setText("conflicting"));
  await act(async () => chet.send());
  expect(requests()[0].state).toBe("send_conflict");
});

it("copies an unknown send back without dropping its reminder; dismissing drops it", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(fail("NETWORK_ERROR"));
  await render();
  await act(async () => chet.setText("first"));
  await act(async () => chet.send());
  const id = vi.mocked(api.submitChetInput).mock.calls[0]![0];
  await act(async () => chet.setText("draft in progress"));
  await act(async () => chet.putBack(id));
  expect(chet.draft).toBe("draft in progress\n\nfirst");
  expect(chet.duplicateRisk).toBe(true);
  expect(requests()).toMatchObject([{ id, state: "send_unknown" }]);
  await act(async () => chet.dismiss(id));
  expect(requests()).toEqual([]);
  expect(pendingKeys()).toEqual([]);
});

it("does not let a status read from before the registration call the request unconfirmed", async () => {
  await render();
  const lateStatus = deferred<ApiResponse<ChetStatus>>();
  const lateHistory = deferred<ApiResponse<ChetPage>>();
  vi.mocked(api.fetchChetStatus).mockReturnValueOnce(lateStatus.promise);
  vi.mocked(api.fetchChetHistoryAfter).mockReturnValueOnce(lateHistory.promise);
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(ok("inserted"));
  await act(async () => chet.setText("race"));
  await act(async () => chet.send());
  const id = vi.mocked(api.submitChetInput).mock.calls[0]![0];
  await act(async () => {
    lateStatus.resolve(status());
    lateHistory.resolve(page([inputRecord(id, "race", GROUP_A)], "1"));
  });
  await flush();
  expect(requests()).toMatchObject([{ id, state: "preparing" }]);
  // A status requested after the registration decides.
  await act(async () => vi.advanceTimersByTimeAsync(1_500));
  expect(requests()[0].state).toBe("unconfirmed");
});

it("reads history only after status, so a saved ending of work that left status is included", async () => {
  await render();
  const historyReads = vi.mocked(api.fetchChetHistoryAfter).mock.calls.length;
  const nextStatus = deferred<ApiResponse<ChetStatus>>();
  vi.mocked(api.fetchChetStatus).mockReturnValueOnce(nextStatus.promise);
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(api.fetchChetHistoryAfter).toHaveBeenCalledTimes(historyReads);
  await act(async () => nextStatus.resolve(status()));
  await flush();
  expect(api.fetchChetHistoryAfter).toHaveBeenCalledTimes(historyReads + 1);
});

it("sends the group open at send time, and retries the request unchanged", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(fail("NETWORK_ERROR"));
  await render(GROUP_A);
  await act(async () => chet.setText("a"));
  // Switching groups while typing changes what the message is about.
  await render(GROUP_B);
  await act(async () => chet.send());
  const [id, request] = vi.mocked(api.submitChetInput).mock.calls[0]!;
  expect(request).toEqual({ text: "a", context: GROUP_B });
  // A reload must retry the same request.
  expect(
    parseChetPending(id, window.localStorage.getItem(CHET_PENDING_PREFIX + id))?.request,
  ).toEqual(request);

  // The open group changed again; a retry still repeats what may have registered.
  await render(null);
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(ok("existing"));
  await act(async () => chet.retry(id));
  expect(vi.mocked(api.submitChetInput).mock.calls[1]).toEqual([id, request]);
});

it("sends a request again about the group it was asked about", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValue(ok("inserted"));
  await render(GROUP_B);
  await act(async () => chet.sendAgain("original", { text: "again", context: GROUP_A }));
  const [id, request] = vi.mocked(api.submitChetInput).mock.calls[0]!;
  expect(request).toEqual({ text: "again", context: GROUP_A });
  expect(requests()).toMatchObject([{ id, state: "preparing" }]);
});

it("does not clear a draft another tab stored before this page heard of it", async () => {
  vi.mocked(api.submitChetInput).mockResolvedValueOnce(ok("inserted"));
  await render();
  await act(async () => chet.setText("mine"));
  // The other tab's write, whose storage event has not arrived yet.
  window.localStorage.setItem(CHET_DRAFT_KEY, "theirs");
  await act(async () => chet.send());
  expect(vi.mocked(api.submitChetInput).mock.calls[0]![1]).toMatchObject({ text: "mine" });
  expect(window.localStorage.getItem(CHET_DRAFT_KEY)).toBe("theirs");
  expect(chet.draft).toBe("theirs");
});

it("takes the draft another tab stored", async () => {
  await render();
  await act(async () => {
    window.dispatchEvent(
      new StorageEvent("storage", { key: CHET_DRAFT_KEY, newValue: "from the other tab" }),
    );
  });
  expect(chet.draft).toBe("from the other tab");
});

it("shows an earlier page's send and answer from its lookup, once, when newer history fills the page", async () => {
  window.localStorage.setItem(
    `${CHET_PENDING_PREFIX}old`,
    JSON.stringify({
      request: { text: "before reload", context: null },
      state: "registered",
      createdAt: "t",
    }),
  );
  const answer: ChetRecord = {
    kind: "result",
    id: "r-old",
    created_at: "2026-11-01T10:01:00.000000Z",
    input_ids: ["old"],
    outcome: { generation: "g1", turn_id: "t", status: "completed", text: "answer", error: "" },
  };
  const newer: ChetRecord = {
    kind: "transcript",
    id: "tr",
    created_at: "2026-11-01T11:00:00.000000Z",
    transcript: {
      call_id: "c",
      turn_id: "u",
      seq: 1,
      role: "user",
      text: "a later call",
      final_update: true,
      complete: true,
    },
  };
  vi.mocked(api.fetchChetHistory).mockResolvedValueOnce(
    ok({
      records: [newer],
      sources: new Map(),
      skipped: 0,
      before: "5",
      after: "9",
      has_more: false,
    }),
  );
  vi.mocked(api.fetchChetInput).mockResolvedValueOnce(
    facts([inputRecord("old", "before reload"), answer]),
  );
  await render();
  await flush();
  expect(pendingKeys()).toEqual([]);
  expect(requests()).toMatchObject([{ id: "old", text: "before reload", state: "completed" }]);
  // The earlier page, once loaded, holds the same facts; they show once, in place.
  vi.mocked(api.fetchChetHistory).mockResolvedValueOnce(
    page([inputRecord("old", "before reload"), answer]),
  );
  await act(async () => chet.loadOlder());
  expect(requests().map((item) => item.id)).toEqual(["old"]);
  expect(chet.items[0]).toMatchObject({ id: "old" });
});

it("asks by ID about a send an earlier page stored", async () => {
  window.localStorage.setItem(
    `${CHET_PENDING_PREFIX}old`,
    JSON.stringify({
      request: { text: "before reload", context: null },
      state: "unknown",
      createdAt: "t",
    }),
  );
  vi.mocked(api.fetchChetInput).mockResolvedValueOnce(facts([inputRecord("old", "before reload")]));
  await render();
  await flush();
  expect(api.fetchChetInput).toHaveBeenCalledWith("old");
  expect(pendingKeys()).toEqual([]);
});

it("gives a not-started input a stable valid successor without secure-context APIs", () => {
  const id = "e692a83f-a26e-47ff-92d7-57ce2bd090c1";
  const successor = resendId(id);
  expect(successor).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  expect(resendId(id.toUpperCase())).toBe(successor);
  expect(resendId(successor)).not.toBe(successor);
  expect(resendId("11111111-1111-1111-8111-111111111111")).not.toBe(
    resendId("11111111-1111-4111-8111-111111111111"),
  );
});

it("allows one successor per not-started request, including after history has reloaded", async () => {
  const original = "e692a83f-a26e-47ff-92d7-57ce2bd090c1";
  const request = { text: "try again", context: GROUP_A };
  const end: ChetRecord = {
    kind: "not_started",
    id: "n1",
    created_at: "2026-11-01T10:00:01Z",
    input_id: original,
    reason: "assistant_unavailable",
  };
  vi.mocked(api.fetchChetHistory).mockResolvedValue(
    page([inputRecord(original, request.text, request.context), end]),
  );
  const response = deferred<ApiResponse<"inserted">>();
  vi.mocked(api.submitChetInput).mockReturnValue(response.promise);
  await render();
  act(() => {
    chet.sendAgain(original, request);
    chet.sendAgain(original, request);
  });
  expect(vi.mocked(api.submitChetInput)).toHaveBeenCalledTimes(1);
  expect(vi.mocked(api.submitChetInput).mock.calls[0]).toEqual([resendId(original), request]);
  await act(async () => response.resolve(ok("inserted")));
  expect(chet.resentIds.has(original)).toBe(true);
  await act(async () => root.unmount());
  host.remove();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(api.fetchChetHistory).mockResolvedValue(
    page([
      inputRecord(original, request.text, request.context),
      end,
      inputRecord(resendId(original), request.text, request.context),
    ]),
  );
  await render();
  expect(chet.resentIds.has(original)).toBe(true);
  await act(async () => chet.sendAgain(original, request));
  expect(vi.mocked(api.submitChetInput)).toHaveBeenCalledTimes(1);
});

it("uses one registration identity when two tabs resend the same input", async () => {
  let first!: ChetConversation, second!: ChetConversation;
  function TwoTabs() {
    first = useChetConversation({ enabled: true, open: true, currentContext: GROUP_A });
    second = useChetConversation({ enabled: true, open: true, currentContext: GROUP_B });
    return null;
  }
  const registered = new Set<string>();
  let dispatches = 0;
  vi.mocked(api.submitChetInput).mockImplementation(async (id) => {
    if (registered.has(id)) return ok("existing");
    registered.add(id);
    dispatches++;
    return ok("inserted");
  });
  await act(async () => root.render(<TwoTabs />));
  const request = { text: "again", context: GROUP_A };
  await act(async () => {
    first.sendAgain("source", request);
    second.sendAgain("source", request);
  });
  expect(vi.mocked(api.submitChetInput)).toHaveBeenCalledTimes(2);
  expect(vi.mocked(api.submitChetInput).mock.calls.map(([id]) => id)).toEqual([
    resendId("source"),
    resendId("source"),
  ]);
  expect(dispatches).toBe(1);
  expect(first.items.find((item) => item.id === resendId("source"))).toMatchObject({
    state: "preparing",
  });
  expect(second.items.find((item) => item.id === resendId("source"))).toMatchObject({
    state: "preparing",
  });
});

it("can retry a resend that was definitely refused before registration", async () => {
  const request = { text: "again", context: GROUP_A };
  vi.mocked(api.submitChetInput)
    .mockResolvedValueOnce(fail("read_only"))
    .mockResolvedValueOnce(ok("inserted"));
  await render();
  await act(async () => chet.sendAgain("source", request));
  await act(async () => chet.sendAgain("source", request));
  expect(vi.mocked(api.submitChetInput).mock.calls.map(([id]) => id)).toEqual([
    resendId("source"),
    resendId("source"),
  ]);
});

it("keeps the newest reading of a Group message, which a later fact about it brings", async () => {
  const input: ChetRecord = {
    kind: "group_input",
    id: "gi",
    created_at: "2026-11-01T10:00:00.000000Z",
    input: {
      group_id: "g-a",
      group_generation: "gen-a",
      event_id: "e-ask",
      sender: { kind: "user" },
      reply_to: null,
    },
  };
  const message = {
    status: "available" as const,
    group_title: "Design",
    sender_title: "",
    text: "@chet what changed?",
    sent_at: "2026-11-01T10:00:00Z",
    has_attachments: false,
    truncated: false,
    reply_event_id: null,
  };
  vi.mocked(api.fetchChetHistory).mockResolvedValueOnce(
    page([input], "1", new Map([["gi", message]])),
  );
  await render();
  expect(requests()).toMatchObject([{ id: "gi", source: "group", state: "waiting" }]);

  const answer: ChetRecord = {
    kind: "result",
    id: "res",
    created_at: "2026-11-01T10:02:00.000000Z",
    input_ids: ["gi"],
    outcome: { generation: "g1", turn_id: "t1", status: "completed", text: "Replied", error: "" },
  };
  vi.mocked(api.fetchChetHistoryAfter)
    .mockResolvedValueOnce(
      page([answer], "2", new Map([["gi", { ...message, reply_event_id: "e-r" }]])),
    )
    .mockResolvedValueOnce(page([], "2"));
  await act(async () => vi.advanceTimersByTimeAsync(1_500));
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(vi.mocked(api.fetchChetHistoryAfter)).toHaveBeenCalledTimes(2);
  expect(requests()).toMatchObject([
    { id: "gi", state: "completed", group: { message: { reply_event_id: "e-r" } } },
  ]);
});

it("asks again about an earlier request after a failed read, at the next status read", async () => {
  const answer: ChetRecord = {
    kind: "result",
    id: "res",
    created_at: "2026-11-01T10:02:00.000000Z",
    input_ids: ["old"],
    outcome: { generation: "g1", turn_id: "t1", status: "completed", text: "Done", error: "" },
  };
  vi.mocked(api.fetchChetHistory).mockResolvedValueOnce(page([answer], "1"));
  vi.mocked(api.fetchChetInput)
    .mockResolvedValueOnce(fail("unavailable"))
    .mockResolvedValueOnce(facts([inputRecord("old", "from an earlier page"), answer]));
  await render();
  expect(requests()).toMatchObject([{ id: "old", sourceMissing: true }]);
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(api.fetchChetInput).toHaveBeenCalledTimes(2);
  expect(requests()).toMatchObject([
    { id: "old", sourceMissing: false, text: "from an earlier page" },
  ]);
});

it("keeps a newer reading of a Group message when an earlier read returns after it", async () => {
  const input: ChetRecord = {
    kind: "group_input",
    id: "gi",
    created_at: "2026-11-01T10:00:00.000000Z",
    input: {
      group_id: "g-a",
      group_generation: "gen-a",
      event_id: "e-ask",
      sender: { kind: "actor", actor_id: "peer-1" },
      reply_to: null,
    },
  };
  const message = {
    status: "available" as const,
    group_title: "Design",
    sender_title: "Peer",
    text: "@chet check this",
    sent_at: "2026-11-01T10:00:00Z",
    has_attachments: false,
    truncated: false,
    reply_event_id: null,
  };
  const dispatched: ChetRecord = {
    kind: "group_dispatch",
    id: "gd",
    created_at: "t",
    input_id: "gi",
  };
  const answer: ChetRecord = {
    kind: "result",
    id: "res",
    created_at: "2026-11-01T10:02:00.000000Z",
    input_ids: ["gi"],
    outcome: { generation: "g1", turn_id: "t1", status: "completed", text: "Replied", error: "" },
  };
  // The first page starts after the input, so the hook reads it by ID; that read is slow.
  const slow = deferred<ApiResponse<{ records: ChetRecord[]; sources: ChetPage["sources"] }>>();
  vi.mocked(api.fetchChetHistory).mockResolvedValueOnce(page([dispatched], "1"));
  vi.mocked(api.fetchChetInput).mockReturnValueOnce(slow.promise);
  await render();
  vi.mocked(api.fetchChetHistoryAfter).mockResolvedValueOnce(
    page([answer], "2", new Map([["gi", { ...message, reply_event_id: "e-r" }]])),
  );
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(api.fetchChetHistoryAfter).toHaveBeenCalledTimes(1);
  await act(async () => {
    slow.resolve(ok({ records: [input, dispatched], sources: new Map([["gi", message]]) }));
  });
  await flush();
  expect(requests()).toMatchObject([
    {
      id: "gi",
      state: "completed",
      text: "@chet check this",
      group: { message: { reply_event_id: "e-r" } },
    },
  ]);
});

/** History as the host keeps it: cursors are positions, and each read answers from them. */
function fakeHistory(initial: ChetRecord[]) {
  const records = [...initial];
  vi.mocked(api.fetchChetHistory).mockImplementation(async (options = {}) => {
    const end = options.before ? Number(options.before) : records.length;
    const start = Math.max(0, end - (options.limit ?? 50));
    return ok({
      records: records.slice(start, end),
      sources: new Map(),
      skipped: 0,
      before: start > 0 ? String(start) : null,
      after: String(records.length),
      has_more: false,
    });
  });
  vi.mocked(api.fetchChetHistoryAfter).mockImplementation(async (after) => {
    const from = Number(after);
    if (!Number.isInteger(from) || from > records.length) return fail("invalid_args");
    const page = records.slice(from, from + 50);
    const next = from + page.length;
    return ok({
      records: page,
      sources: new Map(),
      skipped: 0,
      before: null,
      after: String(next),
      has_more: next < records.length,
    });
  });
  return { append: (...more: ChetRecord[]) => records.push(...more) };
}

const answered = (n: number): ChetRecord => ({
  kind: "result",
  id: `r${n}`,
  created_at: "2026-11-01T10:02:00.000000Z",
  input_ids: [`in${n}`],
  outcome: { generation: "g1", turn_id: `t${n}`, status: "completed", text: "Done", error: "" },
});
const askedInGroup = (n: number): ChetRecord => ({
  kind: "group_input",
  id: `gi${n}`,
  created_at: "2026-11-01T10:05:00.000000Z",
  input: {
    group_id: "g-a",
    group_generation: "gen-a",
    event_id: `e${n}`,
    sender: { kind: "actor", actor_id: "peer-1" },
    reply_to: null,
  },
});
const spoken = (n: number): ChetRecord => ({
  kind: "transcript",
  id: `t${n}`,
  // The same time for every record: order comes from history, not clocks.
  created_at: "2026-11-01T10:05:00.000000Z",
  transcript: {
    call_id: "c",
    turn_id: `t${n}`,
    seq: n,
    role: "user",
    text: "…",
    final_update: true,
    complete: true,
  },
});
const SEEN = "cccc.chet.seen.v2";

it("marks what is new since the conversation was last open, including before it ever was", async () => {
  const history = fakeHistory([answered(1)]);
  await render(GROUP_A, false, true);
  await act(async () => vi.advanceTimersByTimeAsync(3_000));
  expect(chet.unread).toBe(true);

  await render(GROUP_A, true, true);
  expect(chet.unread).toBe(false);
  expect(window.localStorage.getItem(SEEN)).toBe("1");
  await render(GROUP_A, false, true);
  await act(async () => vi.advanceTimersByTimeAsync(3_000));
  expect(chet.unread).toBe(false);

  history.append(askedInGroup(1));
  await act(async () => vi.advanceTimersByTimeAsync(15_000));
  expect(chet.unread).toBe(true);
});

it.each([
  ["a Group message", true],
  ["only other history", false],
])("reads on from what was seen past a full page of history to find %s", async (_, unread) => {
  window.localStorage.setItem(SEEN, "1");
  fakeHistory([
    answered(1),
    ...(unread ? [askedInGroup(1)] : []),
    ...Array.from({ length: 60 }, (_, index) => spoken(index)),
  ]);
  await render(GROUP_A, false, true);
  await act(async () => vi.advanceTimersByTimeAsync(3_000));
  await flush();
  expect(chet.unread).toBe(unread);
});

it("keeps what another tab has read past, and forgets a place history no longer has", async () => {
  window.localStorage.setItem(SEEN, "60");
  fakeHistory([answered(1), askedInGroup(1)]);
  await render(GROUP_A, true, true);
  expect(window.localStorage.getItem(SEEN)).toBe("60");

  // Closed, the stored place is refused: history was made again, and all of it is new here.
  await render(GROUP_A, false, true);
  await act(async () => vi.advanceTimersByTimeAsync(3_000));
  await flush();
  expect(window.localStorage.getItem(SEEN)).toBeNull();
  expect(chet.unread).toBe(true);
});

it("reads on after a failed read, without starting over", async () => {
  window.localStorage.setItem(SEEN, "1");
  const history = fakeHistory([answered(1), spoken(1)]);
  // The first read on from what was seen fails.
  vi.mocked(api.fetchChetHistoryAfter).mockResolvedValueOnce(fail("unavailable"));
  await render(GROUP_A, false, true);
  await act(async () => vi.advanceTimersByTimeAsync(3_000));
  expect(chet.unread).toBe(false);
  history.append(askedInGroup(1));
  await act(async () => vi.advanceTimersByTimeAsync(15_000));
  expect(chet.unread).toBe(true);
  // Reads after the first go on from where history had been read to.
  expect(
    vi.mocked(api.fetchChetHistoryAfter).mock.calls.filter(([after]) => after === "1").length,
  ).toBeLessThanOrEqual(3);
});
