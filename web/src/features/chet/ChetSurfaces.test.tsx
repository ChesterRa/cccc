// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { ChetGroupContext } from "../../services/api/chet";
import { ChetComposer } from "./ChetComposer";
import type { VoiceConversationTurn } from "../codexVoice/codexVoiceProtocol";
import type { ChetRequestItem, ChetTimelineItem, ChetTranscriptItem } from "./chetTimeline";
import { ChetTimeline, type ChetRequestActions } from "./ChetTimeline";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}:${JSON.stringify(options)}` : key,
  }),
}));
vi.mock("../../components/LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: ({ content }: { content: string }) => <div>{content}</div>,
}));

const GROUP_A: ChetGroupContext = { group_id: "g-a", group_title: "Design", scope_key: null };

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function composer(draft: string, extra: Partial<Parameters<typeof ChetComposer>[0]> = {}) {
  const props = {
    draft,
    storageAvailable: true,
    problem: null,
    duplicateRisk: false,
    onText: vi.fn(),
    onSend: vi.fn(),
    onDismissProblem: vi.fn(),
    onReviewRecovery: vi.fn(),
    ...extra,
  };
  act(() => root.render(<ChetComposer {...props} />));
  return props;
}

const buttonByText = (text: string) =>
  [...host.querySelectorAll("button")].find((button) => button.textContent?.includes(text));
const marks = (label: string) => host.querySelectorAll(`[role="img"][aria-label="${label}"]`);

function key(init: KeyboardEventInit & { isComposing?: boolean }) {
  const field = host.querySelector("textarea")!;
  act(() => {
    field.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  });
}

it("sends with Ctrl or Cmd + Enter only, never while an input method composes", () => {
  const props = composer("hello");
  key({ key: "Enter" });
  key({ key: "Enter", ctrlKey: true, isComposing: true });
  expect(props.onSend).not.toHaveBeenCalled();
  key({ key: "Enter", ctrlKey: true });
  key({ key: "Enter", metaKey: true });
  expect(props.onSend).toHaveBeenCalledTimes(2);
  expect(buttonByText("chet.composer.send")?.disabled).toBe(false);
});

it("cannot send an empty message", () => {
  composer("  ");
  expect(buttonByText("chet.composer.send")?.disabled).toBe(true);
});

it("says before sending that this browser cannot keep sends", () => {
  composer("", { storageAvailable: false });
  expect(host.textContent).toContain("chet.composer.storageUnavailable");
});

it("explains a refusal and opens the process review it needs", () => {
  const props = composer("start", {
    problem: { code: "codex_voice_recovery_pending", message: "x", recoveryOwners: ["daemon"] },
  });
  expect(host.textContent).toContain("chet.errors.recoveryPending");
  act(() => buttonByText("codexVoiceReviewRecoveryRecords")!.click());
  expect(props.onReviewRecovery).toHaveBeenCalledWith(["daemon"]);
});

function request(overrides: Partial<ChetRequestItem>): ChetRequestItem {
  return {
    kind: "request",
    id: "r1",
    source: "text",
    createdAt: "2026-11-01T10:00:00.000000Z",
    text: "a question",
    context: null,
    state: "completed",
    outcome: null,
    resultAt: "",
    outcomeShownLater: false,
    notStartedReason: "",
    sourceMissing: false,
    group: null,
    ...overrides,
  };
}

function timeline(
  items: ChetTimelineItem[],
  extra: Partial<ChetRequestActions> = {},
  liveTurns: VoiceConversationTurn[] = [],
  liveCallId: string | null = null,
  onOpenGroupMessage?: (groupId: string, eventId: string) => void,
) {
  const actions: ChetRequestActions = {
    problems: new Map(),
    storageAvailable: true,
    sendAgainDisabled: false,
    resentIds: new Set(),
    onSendAgain: vi.fn(),
    onRetry: vi.fn(),
    onPutBack: vi.fn(),
    onDismiss: vi.fn(),
    onReviewRecovery: vi.fn(),
    ...extra,
  };
  act(() =>
    root.render(
      <ChetTimeline
        items={items}
        liveTurns={liveTurns}
        liveCallId={liveCallId}
        loaded
        hasOlder={false}
        loadingOlder={false}
        onLoadOlder={vi.fn()}
        isDark={false}
        actions={actions}
        onOpenGroupMessage={onOpenGroupMessage}
      />,
    ),
  );
  return actions;
}

function groupRequest(
  message: NonNullable<ChetRequestItem["group"]>["message"],
  overrides: Partial<ChetRequestItem> = {},
): ChetRequestItem {
  return request({
    id: "gi",
    source: "group",
    text: message?.status === "available" ? message.text : "",
    group: {
      input: {
        group_id: "g-a",
        group_generation: "gen-a",
        event_id: "e-ask",
        sender: { kind: "actor", actor_id: "peer-1" },
        reply_to: null,
      },
      message,
    },
    ...overrides,
  });
}

const groupMessage = (overrides: Record<string, unknown> = {}) => ({
  status: "available" as const,
  group_title: "Design",
  sender_title: "Peer One",
  text: "Can you check the release notes?",
  sent_at: "2026-11-01T10:00:00.000000Z",
  has_attachments: false,
  truncated: false,
  reply_event_id: null,
  ...overrides,
});

it("quotes a Group message where it was sent and opens it, or its reply, there", () => {
  const open = vi.fn();
  timeline(
    [
      groupRequest(
        groupMessage({ truncated: true, has_attachments: true, reply_event_id: "e-r" }),
        {
          outcome: {
            generation: "g1",
            turn_id: "t1",
            status: "completed",
            text: "Checked and replied.",
            error: "",
          },
        },
      ),
    ],
    {},
    [],
    null,
    open,
  );
  const entry = host.querySelector('[data-chet-group-message="available"]')!;
  expect(entry.textContent).toContain("Peer One");
  expect(entry.textContent).toContain("Design");
  expect(entry.textContent).toContain("Can you check the release notes?");
  expect(entry.textContent).toContain("chet.groupMessageTruncated · chet.groupMessageAttachments");
  expect(host.textContent).toContain("Checked and replied.");
  expect(host.textContent).toContain("chet.repliedInGroup");
  act(() => buttonByText("chet.openInGroup")!.click());
  act(() => buttonByText("chet.openReply")!.click());
  expect(open.mock.calls).toEqual([
    ["g-a", "e-ask"],
    ["g-a", "e-r"],
  ]);
});

it("says when a finished Group request shows no reply, and offers no link to a gone message", () => {
  timeline([groupRequest(groupMessage())], {}, [], null, vi.fn());
  expect(host.textContent).toContain("chet.noGroupReply");
  timeline([groupRequest(groupMessage(), { state: "working" })], {}, [], null, vi.fn());
  expect(host.textContent).not.toContain("chet.noGroupReply");
  timeline([groupRequest({ status: "unavailable" }, { state: "waiting" })], {}, [], null, vi.fn());
  expect(host.querySelector('[data-chet-group-message="unavailable"]')?.textContent).toContain(
    "chet.groupMessageUnavailable",
  );
  expect(host.textContent).toContain("peer-1");
  expect(host.textContent).toContain("chet.state.waiting");
  expect(buttonByText("chet.openInGroup")).toBeUndefined();
});

it("says a Group message whose sender or source became invalid was not handed to Chet", () => {
  timeline([
    groupRequest(groupMessage(), { state: "not_started", notStartedReason: "source_unavailable" }),
  ]);
  expect(host.textContent).toContain("chet.state.notStartedSourceUnavailable");
  expect(buttonByText("chet.sendAgain")).toBeUndefined();
  expect(host.textContent).not.toContain("chet.noGroupReply");
});

it("greets on first open", () => {
  timeline([]);
  expect(host.textContent).toContain("chet.emptyTitle");
});

it("acts on an unknown send by its own ID", () => {
  const actions = timeline([request({ id: "u1", state: "send_unknown" })]);
  expect(host.textContent).toContain("chet.state.send_unknown");
  act(() => buttonByText("chet.retry")!.click());
  act(() => buttonByText("chet.putBack")!.click());
  act(() => buttonByText("chet.dismiss")!.click());
  expect(actions.onRetry).toHaveBeenCalledWith("u1");
  expect(actions.onPutBack).toHaveBeenCalledWith("u1");
  expect(actions.onDismiss).toHaveBeenCalledWith("u1");
});

it("sends a request that never started again as a new one, unless an unknown send waits", () => {
  const item = request({
    state: "not_started",
    notStartedReason: "assistant_unavailable",
    context: GROUP_A,
  });
  const actions = timeline([item]);
  expect(host.textContent).toContain("chet.state.notStartedUnavailable");
  act(() => buttonByText("chet.sendAgain")!.click());
  expect(actions.onSendAgain).toHaveBeenCalledWith(item.id, {
    text: "a question",
    context: GROUP_A,
  });
  timeline([item], { sendAgainDisabled: true });
  expect(buttonByText("chet.sendAgain")?.disabled).toBe(true);
  timeline([item], { resentIds: new Set([item.id]) });
  expect(buttonByText("chet.resent")?.disabled).toBe(true);
  timeline([{ ...item, sourceMissing: true }]);
  expect(buttonByText("chet.sendAgain")?.disabled).toBe(true);
});

it("names a cancelled queue entry and a too-long answer for what they are", () => {
  timeline([
    request({ id: "c", state: "not_started", notStartedReason: "cancelled" }),
    request({ id: "t", state: "too_large" }),
  ]);
  expect(host.textContent).toContain("chet.state.notStartedCancelled");
  expect(host.textContent).toContain("chet.state.too_large");
  expect(buttonByText("chet.sendAgain")).toBeDefined();
});

it("shows a voice request as the voice model's, with a neutral pending state", () => {
  timeline([request({ source: "voice", state: "unconfirmed", text: "check the release" })]);
  // Standing alone it is named in words, so it does not read as something Chet said.
  expect(host.textContent).toContain("chet.voiceDelegation");
  expect(host.textContent).toContain("check the release");
  expect(host.textContent).toContain("chet.state.voicePending");
  expect(host.textContent).not.toContain("chet.state.unconfirmed");
  expect(buttonByText("chet.sendAgain")).toBeUndefined();
});

it("says when a call's turn was not saved, or its saving is unconfirmed", () => {
  const turn = (id: string, save?: VoiceConversationTurn["save"]): VoiceConversationTurn => ({
    id,
    role: "user",
    text: id,
    final: true,
    callId: "c",
    ...(save ? { save } : {}),
  });
  timeline([], {}, [turn("a", "pending"), turn("b", "saved"), turn("c")]);
  expect(host.textContent).not.toContain("chet.transcriptNotSaved");
  expect(host.textContent).not.toContain("chet.transcriptSaveUnconfirmed");
  timeline([], {}, [turn("d", "not_saved"), turn("e", "unconfirmed")]);
  expect(host.textContent).toContain("chet.transcriptNotSaved");
  expect(host.textContent).toContain("chet.transcriptSaveUnconfirmed");
});

it("lets a shown answer say the request finished, and says it when nothing is shown", () => {
  const outcome = { generation: "g", turn_id: "t", status: "completed", text: "", error: "" };
  timeline([request({ id: "a", outcome: { ...outcome, text: "The answer" } })]);
  expect(host.textContent).toContain("The answer");
  expect(host.textContent).not.toContain("chet.state.completed");
  expect(host.querySelector('[data-chet-state="completed"]')).not.toBeNull();
  timeline([request({ id: "b", outcome })]);
  expect(host.textContent).toContain("chet.state.completed");
});

it("shows the answer and a failure's error", () => {
  timeline([
    request({
      state: "failed",
      outcome: {
        generation: "g",
        turn_id: "t",
        status: "failed",
        text: "",
        error: "Runtime said no",
      },
    }),
  ]);
  expect(host.textContent).toContain("Runtime said no");
  expect(host.textContent).toContain("chet.state.failed");
});

function turn(id: string, role: "user" | "assistant", text: string): ChetTranscriptItem {
  return {
    kind: "transcript",
    id,
    createdAt: "2026-11-01T10:05:00Z",
    callId: "c",
    role,
    text,
    final: true,
    complete: true,
    seq: 0,
  };
}

it("keeps Chet's wording of an answered voice request one click behind the answer", () => {
  const outcome = {
    generation: "g",
    turn_id: "t",
    status: "completed",
    text: "Rain, 14-19",
    error: "",
  };
  timeline([request({ source: "voice", text: "check tomorrow's weather", outcome })]);
  expect(host.textContent).toContain("Rain, 14-19");
  expect(host.textContent).not.toContain("check tomorrow's weather");
  const toggle = host.querySelector<HTMLButtonElement>(
    '[data-chet-answer] button[aria-label="chet.voiceDelegation"]',
  )!;
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  act(() => toggle.click());
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(host.textContent).toContain("check tomorrow's weather");
  // Opened, the wording is named in words, which a touch screen shows without a tooltip.
  expect(host.textContent).toContain("chet.voiceDelegation");
});

it("sets an answer apart from what Chet said aloud, saved or live", () => {
  const outcome = {
    generation: "g",
    turn_id: "t",
    status: "completed",
    text: "Rain, 14-19",
    error: "",
  };
  const live: VoiceConversationTurn = {
    id: "l1",
    role: "assistant",
    text: "said just now",
    final: false,
    callId: "c",
  };
  timeline(
    [
      request({ source: "voice", text: "check tomorrow's weather", outcome }),
      turn("a1", "assistant", "spoken back"),
    ],
    {},
    [live],
  );
  const answers = host.querySelectorAll("[data-chet-answer]");
  expect(answers).toHaveLength(1);
  expect(answers[0].textContent).toContain("Rain, 14-19");
  // The answer carries no voice mark, its request toggle included.
  expect(answers[0].querySelector('[role="img"]')).toBeNull();
  const spoken = [...host.querySelectorAll("[data-chet-spoken]")];
  expect(spoken).toHaveLength(2);
  for (const line of spoken) {
    expect(line.closest("[data-chet-answer]")).toBeNull();
    expect(line.querySelector('[role="img"]')).not.toBeNull();
  }
  // A live line is a caption like a saved run, but stays open while it is spoken.
  expect(spoken[1].textContent).toContain("said just now");
});

it("marks what was spoken with the voice icon instead of a word, and leaves typed requests unmarked", () => {
  const live: VoiceConversationTurn = {
    id: "l1",
    role: "assistant",
    text: "said just now",
    final: true,
    callId: "c",
  };
  timeline(
    [
      request({ id: "typed", context: GROUP_A }),
      turn("u1", "user", "spoken"),
      turn("a1", "assistant", "spoken back"),
    ],
    {},
    [live],
  );
  const typed = host.querySelector('[data-chet-request="text"]')!;
  expect(typed.textContent).toContain("Design");
  expect(typed.querySelector('[role="img"]')).toBeNull();
  const spoken = [...marks("chet.sourceTranscript")].map((mark) => mark.getAttribute("title"));
  expect(spoken).toEqual(["chet.sourceTranscript", "chet.voiceRunHint", "chet.sourceTranscript"]);
  expect(host.textContent).not.toContain("chet.sourceTranscript");
});

it("folds each run of Chet's voice turns to its last turn, and keeps it open as it grows", () => {
  const items = [
    turn("u1", "user", "what's the weather"),
    turn("a1", "assistant", "let me check"),
    turn("a2", "assistant", "light rain tomorrow"),
    turn("u2", "user", "thanks"),
    turn("a3", "assistant", "you're welcome"),
  ];
  timeline(items);
  expect(host.querySelectorAll("[data-chet-voice-run]")).toHaveLength(2);
  expect(host.textContent).toContain("what's the weather");
  expect(host.textContent).toContain("thanks");
  expect(host.textContent).toContain("light rain tomorrow");
  expect(host.textContent).not.toContain("let me check");
  const run = host.querySelector("[data-chet-voice-run] button")!;
  act(() => (run as HTMLButtonElement).click());
  expect(host.textContent).toContain("let me check");
  timeline([...items.slice(0, 3), turn("a4", "assistant", "and windy"), ...items.slice(3)]);
  expect(host.textContent).toContain("let me check");
  expect(host.textContent).toContain("and windy");
});

it("keeps the call's latest run of Chet's turns open while the call lasts, unless the reader closes it", () => {
  const items = [
    { ...turn("u1", "user", "question"), seq: 1 },
    { ...turn("a1", "assistant", "an earlier reply"), seq: 2 },
    { ...turn("u2", "user", "and then"), seq: 3 },
    { ...turn("a2", "assistant", "still being played"), seq: 4 },
  ];
  const states = () =>
    [...host.querySelectorAll("[data-chet-voice-run] > button")].map((button) =>
      button.getAttribute("aria-expanded"),
    );
  timeline(items, {}, [], "c");
  expect(states()).toEqual(["false", "true"]);
  timeline(items, {}, [], null);
  expect(states()).toEqual(["false", "false"]);
  // An earlier turn saved after a later one does not take the default from it.
  timeline([items[0], items[3], items[2], items[1]], {}, [], "c");
  expect(states()).toEqual(["true", "false"]);
  timeline(items, {}, [], "c");
  act(() => (host.querySelectorAll("[data-chet-voice-run] > button")[1] as HTMLElement).click());
  timeline(items, {}, [], "c");
  expect(states()).toEqual(["false", "false"]);
});

it("says a folded run's transcript is incomplete or unfinished without opening it", () => {
  const cut = { ...turn("a1", "assistant", "the rain will"), complete: false };
  timeline([turn("u1", "user", "weather?"), cut, turn("a2", "assistant", "bring an umbrella")]);
  expect(host.textContent).toContain("chet.transcriptIncomplete");
  timeline([turn("u1", "user", "weather?"), { ...cut, final: false }]);
  expect(host.textContent).toContain("chet.transcriptUnfinished");
  timeline([turn("u1", "user", "weather?"), turn("a2", "assistant", "bring an umbrella")]);
  expect(host.textContent).not.toContain("chet.transcriptIncomplete");
  expect(host.textContent).not.toContain("chet.transcriptUnfinished");
});

it("does not apply an older-page scroll anchor to new entries after a load fails", async () => {
  let finish!: () => void;
  const waiting = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const frames: FrameRequestCallback[] = [];
  const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.push(callback);
    return frames.length;
  });
  const actions: ChetRequestActions = {
    problems: new Map(),
    storageAvailable: true,
    sendAgainDisabled: false,
    resentIds: new Set(),
    onSendAgain: vi.fn(),
    onRetry: vi.fn(),
    onPutBack: vi.fn(),
    onDismiss: vi.fn(),
    onReviewRecovery: vi.fn(),
  };
  const draw = (items: ChetTimelineItem[]) =>
    act(() =>
      root.render(
        <ChetTimeline
          items={items}
          liveTurns={[]}
          loaded
          hasOlder
          loadingOlder={false}
          onLoadOlder={() => waiting}
          isDark={false}
          actions={actions}
        />,
      ),
    );
  const a = request({ id: "a" }),
    b = request({ id: "b" }),
    c = request({ id: "c" });
  draw([a]);
  const view = host.querySelector<HTMLElement>("[data-chet-timeline]")!;
  let height = 1000;
  Object.defineProperty(view, "scrollHeight", { configurable: true, get: () => height });
  Object.defineProperty(view, "clientHeight", { configurable: true, value: 400 });
  view.scrollTop = 200;
  act(() => view.dispatchEvent(new Event("scroll")));
  act(() => buttonByText("chet.loadOlder")!.click());
  height = 1100;
  draw([a, b]);
  expect(view.scrollTop).toBe(200);
  await act(async () => {
    finish();
    await Promise.resolve();
  });
  act(() => frames.splice(0).forEach((frame) => frame(performance.now())));
  height = 1200;
  draw([a, b, c]);
  expect(view.scrollTop).toBe(200);
  raf.mockRestore();
});

it("keeps the reader's place when an earlier page arrives", () => {
  const actions: ChetRequestActions = {
    problems: new Map(),
    storageAvailable: true,
    sendAgainDisabled: false,
    resentIds: new Set(),
    onSendAgain: vi.fn(),
    onRetry: vi.fn(),
    onPutBack: vi.fn(),
    onDismiss: vi.fn(),
    onReviewRecovery: vi.fn(),
  };
  const pending = new Promise<void>(() => {});
  const draw = (items: ChetTimelineItem[]) =>
    act(() =>
      root.render(
        <ChetTimeline
          items={items}
          liveTurns={[]}
          loaded
          hasOlder
          loadingOlder={false}
          onLoadOlder={() => pending}
          isDark={false}
          actions={actions}
        />,
      ),
    );
  const current = request({ id: "current" });
  const existing = request({ id: "existing" });
  draw([current, existing]);
  const view = host.querySelector<HTMLElement>("[data-chet-timeline]")!;
  let height = 1000;
  Object.defineProperty(view, "scrollHeight", { configurable: true, get: () => height });
  Object.defineProperty(view, "clientHeight", { configurable: true, value: 400 });
  view.scrollTop = 200;
  act(() => view.dispatchEvent(new Event("scroll")));
  act(() => buttonByText("chet.loadOlder")!.click());
  // A voice delegation can move when its result arrives. It is not an older page.
  draw([existing, current]);
  expect(view.scrollTop).toBe(200);
  height = 1100;
  const newer = request({ id: "newer" });
  draw([existing, current, newer]);
  expect(view.scrollTop).toBe(200);
  height = 1200;
  draw([request({ id: "older" }), existing, current, newer]);
  expect(view.scrollTop).toBe(300);
});
