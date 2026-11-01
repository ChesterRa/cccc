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

const GROUP_A: ChetGroupContext = { group_id: "g-a", group_title: "Design", scope_key: "s1" };
const GROUP_B: ChetGroupContext = { group_id: "g-b", group_title: "Release", scope_key: null };

let host: HTMLDivElement;
let root: Root;
let chet: ChetConversation;

function Harness(props: { open: boolean; currentContext: ChetGroupContext | null }) {
  chet = useChetConversation({ enabled: true, ...props });
  return null;
}

const ok = <T,>(result: T): ApiResponse<T> => ({ ok: true, result });
const fail = (code: string): ApiResponse<never> => ({ ok: false, error: { code, message: code } });
const page = (records: ChetRecord[] = [], after = "0"): ApiResponse<ChetPage> =>
  ok({ records, skipped: 0, before: null, after, has_more: false });
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

async function render(currentContext: ChetGroupContext | null = GROUP_A, open = true) {
  await act(async () => root.render(<Harness open={open} currentContext={currentContext} />));
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
  vi.mocked(api.fetchChetInput).mockResolvedValue(ok([]));
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
  await act(async () => chet.sendAgain({ text: "again", context: GROUP_A }));
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
    ok({ records: [newer], skipped: 0, before: "5", after: "9", has_more: false }),
  );
  vi.mocked(api.fetchChetInput).mockResolvedValueOnce(
    ok([inputRecord("old", "before reload"), answer]),
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
  vi.mocked(api.fetchChetInput).mockResolvedValueOnce(ok([inputRecord("old", "before reload")]));
  await render();
  await flush();
  expect(api.fetchChetInput).toHaveBeenCalledWith("old");
  expect(pendingKeys()).toEqual([]);
});
