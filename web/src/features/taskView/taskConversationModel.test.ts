import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import {
  belongsToTask,
  collectTaskConversation,
  mentionsTask,
  withTaskViewRef,
} from "./taskConversationModel";

let clock = 0;
function message(id: string, text: string, extra: Record<string, unknown> = {}): LedgerEvent {
  clock += 1;
  return {
    id,
    kind: "chat.message",
    group_id: "g1",
    by: "worker",
    ts: `2026-11-27T00:00:${String(clock).padStart(2, "0")}Z`,
    data: { text, ...extra },
  };
}
const ref = (taskId: string) => ({ refs: [{ kind: "task_ref", task_id: taskId }] });
const ids = (events: LedgerEvent[]) => events.map((event) => event.id);

describe("which messages name a task", () => {
  it.each([
    ["T98 is done", true],
    ["已完成T98。", true],
    ["see (t98)", true],
    ["T981 is done", false],
    ["T9810", false],
    ["XT98", false],
    ["commit a1T98ff0", false],
    ["T98_draft", false],
    ["nothing here", false],
  ])("%s -> %s", (text, expected) => {
    expect(mentionsTask(message("m", text), "T98")).toBe(expected);
  });

  it("counts a structured reference even when the text never names the task", () => {
    expect(belongsToTask(message("m", "status update", ref("T98")), "T98")).toBe(true);
    expect(belongsToTask(message("m", "status update", ref("T981")), "T98")).toBe(false);
  });

  it("does not match the ID inside other fields, as the server search does", () => {
    // The server finds "T98" anywhere in the record; only text and refs decide here.
    const event = message("m", "hello", { quote_text: "T98", client_id: "T98" });
    expect(belongsToTask(event, "T98")).toBe(false);
  });
});

describe("a task's conversation", () => {
  it("joins search results with loaded messages and follows reply chains both ways", () => {
    const question = message("q", "how should we do this?");
    const tagged = message("a", "tracking it", { ...ref("T98"), reply_to: "q" });
    const reply = message("r", "sounds good", { reply_to: "a" });
    const replyToReply = message("rr", "agreed", { reply_to: "r" });
    const unrelated = message("u", "lunch?");
    const old = message("old", "T98 kickoff");
    const view = collectTaskConversation(
      [old, tagged],
      [question, tagged, reply, replyToReply, unrelated],
      "T98",
    );
    expect(ids(view.events).sort()).toEqual(["a", "old", "q", "r", "rr"]);
    expect(view.contextIncomplete).toBe(false);
  });

  it("leaves out other members' replies to the same question", () => {
    const question = message("q", "status, everyone?");
    const mine = message("mine", "T98 is green", { reply_to: "q" });
    const theirs = message("theirs", "billing is blocked", { reply_to: "q" });
    const underTheirs = message("deep", "since Monday", { reply_to: "theirs" });
    const underMine = message("ack", "thanks", { reply_to: "mine" });
    const view = collectTaskConversation(
      [],
      [question, mine, theirs, underTheirs, underMine],
      "T98",
    );
    expect(ids(view.events).sort()).toEqual(["ack", "mine", "q"]);
  });

  it("follows what a task message answers up through every level", () => {
    const root = message("root", "plan?");
    const middle = message("mid", "three steps", { reply_to: "root" });
    const seed = message("seed", "T98 covers step one", { reply_to: "mid" });
    const view = collectTaskConversation([], [root, middle, seed], "T98");
    expect(ids(view.events)).toEqual(["root", "mid", "seed"]);
  });

  it("follows all descendants through a reply also included as a task message's ancestor", () => {
    const root = message("root", "T98 kickoff");
    const middle = message("mid", "details", { reply_to: "root" });
    const seed = message("seed", "T98 update", { reply_to: "mid" });
    const reply = message("reply", "confirmed", { reply_to: "mid" });
    const view = collectTaskConversation([], [root, middle, seed, reply], "T98");
    expect(ids(view.events)).toEqual(["root", "mid", "seed", "reply"]);
  });

  it("orders by time and shows each message once", () => {
    const first = message("1", "T98 one");
    const second = message("2", "T98 two");
    const view = collectTaskConversation([second, first], [first, second], "T98");
    expect(ids(view.events)).toEqual(["1", "2"]);
  });

  it("drops server hits that only contain the ID inside a longer one", () => {
    const view = collectTaskConversation([message("x", "T981 only")], [], "T98");
    expect(view.events).toEqual([]);
  });

  it("says so when a shown message replies to one that is not at hand", () => {
    const orphan = message("a", "T98 follow-up", { reply_to: "gone" });
    const view = collectTaskConversation([orphan], [], "T98");
    expect(ids(view.events)).toEqual(["a"]);
    expect(view.contextIncomplete).toBe(true);
  });

  it("includes a message still being sent, which has no ledger ID yet", () => {
    const pending = { ...message("", "T98 on it"), id: undefined } as unknown as LedgerEvent;
    expect(collectTaskConversation([], [pending], "T98").events).toHaveLength(1);
  });
});

describe("messages sent from a task's view", () => {
  it("carry the task's reference, added once", () => {
    const refs = withTaskViewRef([{ kind: "presentation_ref" }], "T98", "Ship it");
    expect(refs).toEqual([
      { kind: "presentation_ref" },
      { kind: "task_ref", task_id: "T98", title: "Ship it" },
    ]);
    expect(withTaskViewRef(refs, "T98", "Ship it")).toEqual(refs);
  });
});
