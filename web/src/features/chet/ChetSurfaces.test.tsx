// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { ChetGroupContext } from "../../services/api/chet";
import { ChetComposer } from "./ChetComposer";
import type { VoiceConversationTurn } from "../codexVoice/codexVoiceProtocol";
import type { ChetRequestItem } from "./chetTimeline";
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
    ...overrides,
  };
}

function timeline(
  items: ChetRequestItem[],
  extra: Partial<ChetRequestActions> = {},
  liveTurns: VoiceConversationTurn[] = [],
) {
  const actions: ChetRequestActions = {
    problems: new Map(),
    storageAvailable: true,
    sendAgainDisabled: false,
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
        loaded
        hasOlder={false}
        loadingOlder={false}
        onLoadOlder={vi.fn()}
        isDark={false}
        actions={actions}
      />,
    ),
  );
  return actions;
}

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
  expect(actions.onSendAgain).toHaveBeenCalledWith({ text: "a question", context: GROUP_A });
  timeline([item], { sendAgainDisabled: true });
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
  expect(host.textContent).toContain("chet.voiceDelegation");
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
