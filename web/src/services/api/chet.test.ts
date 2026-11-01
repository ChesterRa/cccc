import { afterEach, expect, it, vi } from "vite-plus/test";
import {
  chetSubmissionRefused,
  fetchChetHistory,
  fetchChetHistoryAfter,
  fetchChetInput,
  fetchChetStatus,
  isChetRecord,
  submitChetInput,
} from "./chet";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function respond(body: unknown, status = 200) {
  vi.stubGlobal("window", { location: { search: "" } });
  return vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(new Response(JSON.stringify(body), { status }));
}

const input = {
  kind: "input",
  id: "11111111-1111-4111-8111-111111111111",
  created_at: "2026-11-01T10:00:00.000000Z",
  request: { text: "hello", context: { group_id: "g1", group_title: "Design", scope_key: null } },
};

it("accepts each history fact only in the shape the host writes", () => {
  expect(isChetRecord(input)).toBe(true);
  expect(isChetRecord({ ...input, request: { text: "hello" } })).toBe(false);
  expect(
    isChetRecord({
      kind: "result",
      id: "r",
      created_at: "t",
      input_ids: ["a"],
      outcome: { status: "completed" },
    }),
  ).toBe(false);
  const transcript = {
    kind: "transcript",
    id: "t",
    created_at: "t",
    transcript: {
      call_id: "c",
      turn_id: "u",
      seq: 1,
      role: "user",
      text: "",
      final_update: true,
      complete: false,
    },
  };
  expect(isChetRecord(transcript)).toBe(true);
  // The order is required: without it a call's turns cannot be placed.
  for (const seq of [undefined, 0, 1.5]) {
    expect(isChetRecord({ ...transcript, transcript: { ...transcript.transcript, seq } })).toBe(
      false,
    );
  }
  expect(isChetRecord({ ...input, kind: "future_kind" })).toBe(false);
});

it("counts records it cannot show instead of dropping them silently", async () => {
  const fetch = respond({
    ok: true,
    result: {
      records: [input, { kind: "future_kind", id: "x", created_at: "t" }],
      before: "10",
      after: "99",
    },
  });
  const response = await fetchChetHistory({ limit: 50 });
  expect(new URL(String(fetch.mock.calls[0]![0]), "http://localhost").search).toBe("?limit=50");
  expect(response).toMatchObject({
    ok: true,
    result: { records: [input], skipped: 1, before: "10", after: "99", has_more: false },
  });
});

it("reads appended records after an opaque cursor", async () => {
  const fetch = respond({ ok: true, result: { records: [], after: "120", has_more: true } });
  const response = await fetchChetHistoryAfter("99/+");
  expect(
    new URL(String(fetch.mock.calls[0]![0]), "http://localhost").searchParams.get("after"),
  ).toBe("99/+");
  expect(response).toMatchObject({
    ok: true,
    result: { before: null, after: "120", has_more: true },
  });
});

it("reads an unknown ID as no records, and anything else malformed as an error", async () => {
  respond({ ok: true, result: { records: null } });
  expect(await fetchChetInput("missing")).toEqual({ ok: true, result: [] });
  respond({ ok: true, result: {} });
  expect((await fetchChetInput("missing")).ok).toBe(false);
});

it("rejects a malformed status rather than guessing live work", async () => {
  respond({ ok: true, result: { generation: "g", preparing: [], queued: "a", working: [] } });
  expect((await fetchChetStatus()).ok).toBe(false);
  respond({
    ok: true,
    result: {
      generation: null,
      preparing: ["a"],
      queued: [],
      working: [],
      history_write_failed: false,
    },
  });
  expect(await fetchChetStatus()).toMatchObject({ ok: true, result: { owner_busy: false } });
});

it("posts the ID with the request and reads only the two registrations", async () => {
  const fetch = respond({ ok: true, result: { registration: "existing" } });
  const response = await submitChetInput("id-1", { text: "hi", context: null });
  const [path, init] = fetch.mock.calls[0]!;
  expect(path).toBe("/api/v1/chet/inputs");
  expect(JSON.parse(String(init?.body))).toEqual({
    id: "id-1",
    request: { text: "hi", context: null },
  });
  expect(response).toEqual({ ok: true, result: "existing" });
  respond({ ok: true, result: { registration: "maybe" } });
  expect((await submitChetInput("id-1", { text: "hi", context: null })).ok).toBe(false);
});

it("treats only refusals before registration as certainly not sent", () => {
  for (const code of [
    "invalid_request",
    "permission_denied",
    "read_only",
    "codex_voice_recovery_pending",
  ]) {
    expect(chetSubmissionRefused(code)).toBe(true);
  }
  // Other 400s and lost responses may follow a write.
  for (const code of [
    "chet_history_unknown",
    "chet_history_corrupt",
    "chet_submission_unknown",
    "NETWORK_ERROR",
    "HTTP_ERROR",
  ]) {
    expect(chetSubmissionRefused(code)).toBe(false);
  }
});
