import { describe, expect, it } from "vite-plus/test";
import type { ChetRecord, ChetStatus } from "../../services/api/chet";
import {
  buildChetTimeline,
  missingRequestIds,
  unsavedLiveTurns,
  type ChetLiveView,
  type ChetRequestItem,
} from "./chetTimeline";

const at = (minute: number) => `2026-11-01T10:${String(minute).padStart(2, "0")}:00.000000Z`;

function input(id: string, text = `ask ${id}`, minute = 0): ChetRecord {
  return { kind: "input", id, created_at: at(minute), request: { text, context: null } };
}

function result(
  id: string,
  inputIds: string[],
  status = "completed",
  turn = `turn-${id}`,
): ChetRecord {
  return {
    kind: "result",
    id,
    created_at: at(30),
    input_ids: inputIds,
    outcome: { generation: "g1", turn_id: turn, status, text: `answer ${turn}`, error: "" },
  };
}

function status(overrides: Partial<ChetStatus> = {}): ChetStatus {
  return {
    generation: "g1",
    preparing: [],
    queued: [],
    working: [],
    history_write_failed: false,
    owner_busy: false,
    ...overrides,
  };
}

function view(overrides: Partial<ChetLiveView> = {}): ChetLiveView {
  return { status: status(), assumedLive: new Set(), settledLive: new Set(), ...overrides };
}

function delegation(id: string, minute = 3): ChetRecord {
  return {
    kind: "voice_delegation",
    id,
    created_at: at(minute),
    delegation: { call_id: "c", delegation_id: `d-${id}`, text: `task ${id}` },
  };
}

function receipt(id: string, inputId: string): ChetRecord {
  return {
    kind: "receipt",
    id,
    created_at: at(4),
    input_id: inputId,
    receipt: { generation: "g1", turn_id: "t" },
  };
}

function spoken(id: string, seq: number, role: "user" | "assistant"): ChetRecord {
  return {
    kind: "transcript",
    id,
    created_at: at(5),
    transcript: {
      call_id: "c",
      turn_id: id,
      seq,
      role,
      text: id,
      final_update: true,
      complete: true,
    },
  };
}

/** Each item as `request:<id>` or `<role>:<turn>`, in timeline order. */
const order = (records: ChetRecord[], live = view(), fetched = new Map()) =>
  buildChetTimeline(records, live, [], fetched).map((item) =>
    item.kind === "request" ? `request:${item.id}` : `${item.role}:${item.text}`,
  );

const requests = (records: ChetRecord[], live = view()) =>
  buildChetTimeline(records, live).filter(
    (item): item is ChetRequestItem => item.kind === "request",
  );

describe("buildChetTimeline", () => {
  it("shows only a result record as a terminal state", () => {
    const [done, failed, cancelled, other] = requests([
      input("a"),
      input("b"),
      input("c"),
      input("d"),
      result("ra", ["a"]),
      result("rb", ["b"], "failed"),
      result("rc", ["c"], "cancelled"),
      result("rd", ["d"], "interrupted"),
      input("e"),
      result("re", ["e"], "result_too_large"),
    ]);
    expect(done).toMatchObject({ state: "completed", outcome: { text: "answer turn-ra" } });
    expect(failed.state).toBe("failed");
    expect(cancelled.state).toBe("cancelled");
    expect(other.state).toBe("ended");
    expect(requests([input("e"), result("re", ["e"], "result_too_large")])[0].state).toBe(
      "too_large",
    );
  });

  it("keeps the reason a request never started", () => {
    const [unavailable, cancelled] = requests([
      input("a"),
      input("b"),
      {
        kind: "not_started",
        id: "n1",
        created_at: at(1),
        input_id: "a",
        reason: "assistant_unavailable",
      },
      { kind: "not_started", id: "n2", created_at: at(1), input_id: "b", reason: "cancelled" },
    ]);
    expect(unavailable).toMatchObject({
      state: "not_started",
      notStartedReason: "assistant_unavailable",
    });
    expect(cancelled).toMatchObject({ state: "not_started", notStartedReason: "cancelled" });
  });

  it("derives live phases only from status that can confirm them", () => {
    const records = [input("p"), input("q"), input("w")];
    const live = requests(
      records,
      view({ status: status({ preparing: ["p"], queued: ["q"], working: ["w"] }) }),
    );
    expect(live.map((item) => item.state)).toEqual(["preparing", "queued", "working"]);
  });

  it("separates a received request from one with no evidence, and never claims either finished", () => {
    const [received, silent] = requests([
      input("a"),
      input("b"),
      {
        kind: "receipt",
        id: "r",
        created_at: at(2),
        input_id: "a",
        receipt: { generation: "g1", turn_id: "t" },
      },
    ]);
    expect(received.state).toBe("received_unconfirmed");
    expect(silent.state).toBe("unconfirmed");
  });

  it("rules nothing out while the owner is busy or status is unknown", () => {
    const records = [input("a"), input("b")];
    // A busy owner reports preparation only, even for a request never seen live before.
    const busy = requests(
      records,
      view({ status: status({ owner_busy: true, preparing: ["b"] }) }),
    );
    expect(busy.map((item) => item.state)).toEqual(["checking", "preparing"]);
    expect(requests(records, view({ status: null })).map((item) => item.state)).toEqual([
      "checking",
      "checking",
    ]);
  });

  it("treats a request registered after the status read as still preparing", () => {
    const [item] = requests([input("a")], view({ assumedLive: new Set(["a"]) }));
    expect(item.state).toBe("preparing");
  });

  it("shows one answer for requests that finished in the same turn", () => {
    const [first, second] = requests([
      input("a"),
      input("b"),
      result("ra", ["a"], "completed", "shared"),
      result("rb", ["b"], "completed", "shared"),
    ]);
    expect(first).toMatchObject({ state: "completed", outcomeShownLater: true });
    expect(second).toMatchObject({ state: "completed", outcomeShownLater: false });
  });

  it("keeps a result whose request is on an earlier page", () => {
    const records = [result("ra", ["old"])];
    expect(missingRequestIds(records)).toEqual(["old"]);
    const [placeholder] = requests(records);
    expect(placeholder).toMatchObject({ id: "old", sourceMissing: true, state: "completed" });
    const fetched = new Map([
      ["old", input("old", "the earlier question") as Extract<ChetRecord, { kind: "input" }>],
    ]);
    const [filled] = buildChetTimeline(records, view(), [], fetched) as ChetRequestItem[];
    expect(filled).toMatchObject({ sourceMissing: false, text: "the earlier question" });
  });

  it("shows running work whose request is on an earlier page", () => {
    const receipt: ChetRecord = {
      kind: "receipt",
      id: "rc",
      created_at: at(4),
      input_id: "old",
      receipt: { generation: "g1", turn_id: "t" },
    };
    const live = view({ status: status({ working: ["old", "older"] }) });
    expect(missingRequestIds([input("a"), receipt], live.status)).toEqual(["old", "older"]);
    // At its receipt, or after everything when only status names it.
    const items = requests([input("a"), receipt], live);
    expect(items.map((item) => [item.id, item.state, item.sourceMissing])).toEqual([
      ["a", "unconfirmed", false],
      ["old", "working", true],
      ["older", "working", true],
    ]);
  });

  it("does not repeat this browser's send that status already reports", () => {
    const local = [
      { id: "s", text: "mine", context: null, createdAt: at(9), phase: "registered" as const },
    ];
    const items = buildChetTimeline([], view({ status: status({ queued: ["s"] }) }), local);
    expect(items).toMatchObject([{ id: "s", state: "queued", text: "mine" }]);
  });

  it("marks a voice delegation as the voice model's request", () => {
    const [item] = requests([
      {
        kind: "voice_delegation",
        id: "v",
        created_at: at(3),
        delegation: { call_id: "c", delegation_id: "d", text: "look up the release" },
      },
    ]);
    expect(item).toMatchObject({ source: "voice", text: "look up the release", context: null });
  });

  it("shows a voice delegation where its latest fact is, not before what the user said", () => {
    // The delegation is saved as the voice model hands work off, before the user's words.
    const records = [
      delegation("v"),
      receipt("rc", "v"),
      spoken("question", 1, "user"),
      spoken("ack", 2, "assistant"),
      result("rv", ["v"]),
      spoken("summary", 3, "assistant"),
    ];
    expect(order(records)).toEqual([
      "user:question",
      "assistant:ack",
      "request:v",
      "assistant:summary",
    ]);
    // Without a result, the receipt is the latest fact; nothing reorders it past later turns.
    expect(order(records.slice(0, 4))).toEqual(["request:v", "user:question", "assistant:ack"]);
    const refused: ChetRecord = {
      kind: "not_started",
      id: "n",
      created_at: at(6),
      input_id: "v",
      reason: "assistant_unavailable",
    };
    expect(order([delegation("v"), spoken("question", 1, "user"), refused])).toEqual([
      "user:question",
      "request:v",
    ]);
  });

  it("keeps settled live voice work last, through a busy owner, and states only what is known", () => {
    const records = [delegation("v"), receipt("rc", "v"), spoken("question", 1, "user")];
    const settledLive = new Set(["v"]);
    expect(order(records, view({ status: status({ working: ["v"] }), settledLive }))).toEqual([
      "user:question",
      "request:v",
    ]);
    const busy = view({ status: status({ owner_busy: true }), settledLive });
    expect(order(records, busy)).toEqual(["user:question", "request:v"]);
    expect(requests(records, busy)[0].state).toBe("checking");
    // A free owner that no longer reports it settles it back at its receipt.
    expect(order(records)).toEqual(["request:v", "user:question"]);
    expect(requests(records)[0].state).toBe("received_unconfirmed");
  });

  it("leaves text requests where they were and shows a mixed turn's answer once", () => {
    const records = [
      input("a"),
      delegation("v"),
      spoken("question", 1, "user"),
      result("ra", ["a"], "completed", "shared"),
      result("rv", ["v"], "completed", "shared"),
    ];
    expect(order(records)).toEqual(["request:a", "user:question", "request:v"]);
    const [text, voice] = requests(records);
    expect(text).toMatchObject({ source: "text", outcomeShownLater: true });
    expect(voice).toMatchObject({ source: "voice", outcomeShownLater: false });
  });

  it("places an earlier page's voice delegation by its latest fact once its source is known", () => {
    const records = [receipt("rc", "old"), spoken("question", 1, "user"), result("rv", ["old"])];
    // Until the source is read, it could be a typed request: shown at its first fact.
    expect(order(records)).toEqual(["request:old", "user:question"]);
    const fetched = new Map([
      ["old", delegation("old") as Extract<ChetRecord, { kind: "voice_delegation" }>],
    ]);
    expect(order(records, view(), fetched)).toEqual(["user:question", "request:old"]);
  });

  it("shows each turn once, in the order the call first saw it", () => {
    const transcript = (id: string, turn: string, seq: number, role: "user" | "assistant") =>
      ({
        kind: "transcript",
        id,
        created_at: at(5),
        transcript: {
          call_id: "c",
          turn_id: turn,
          seq,
          role,
          text: turn,
          final_update: true,
          complete: true,
        },
      }) as ChetRecord;
    // The question's transcription finished after the reply it prompted.
    const items = buildChetTimeline(
      [
        transcript("1", "reply", 2, "assistant"),
        transcript("2", "question", 1, "user"),
        transcript("3", "question", 1, "user"),
      ],
      view(),
    );
    expect(items.map((item) => (item.kind === "transcript" ? item.text : ""))).toEqual([
      "question",
      "reply",
    ]);
  });

  it("shows a call's turn once: from history once it is saved, live until then", () => {
    const saved = buildChetTimeline(
      [
        {
          kind: "transcript",
          id: "t",
          created_at: at(5),
          transcript: {
            call_id: "c1",
            turn_id: "u1",
            seq: 1,
            role: "user",
            text: "saved",
            final_update: true,
            complete: true,
          },
        },
      ],
      view(),
    );
    const live = [
      { id: "u1", role: "user" as const, text: "saved", final: true, callId: "c1" },
      { id: "a1", role: "assistant" as const, text: "not yet", final: false, callId: "c1" },
      { id: "u1", role: "user" as const, text: "other call", final: true, callId: "c2" },
    ];
    expect(unsavedLiveTurns(saved, live).map((turn) => turn.text)).toEqual([
      "not yet",
      "other call",
    ]);
  });

  it("adds this browser's sends until history shows them", () => {
    const local = [
      { id: "s", text: "sending", context: null, createdAt: at(9), phase: "sending" as const },
      { id: "u", text: "unknown", context: null, createdAt: at(9), phase: "unknown" as const },
      { id: "c", text: "conflict", context: null, createdAt: at(9), phase: "conflict" as const },
      {
        id: "r",
        text: "registered",
        context: null,
        createdAt: at(9),
        phase: "registered" as const,
      },
      { id: "a", text: "shown", context: null, createdAt: at(9), phase: "registered" as const },
    ];
    const items = buildChetTimeline([input("a")], view(), local) as ChetRequestItem[];
    expect(items.map((item) => [item.id, item.state])).toEqual([
      ["a", "unconfirmed"],
      ["s", "sending"],
      ["u", "send_unknown"],
      ["c", "send_conflict"],
      ["r", "preparing"],
    ]);
  });
});
