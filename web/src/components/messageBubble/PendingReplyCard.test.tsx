// @vitest-environment happy-dom
import { act, useState, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PendingReplyCard } from "./PendingReplyCard";
import type { ReplyDraft } from "../../features/pendingReplies/types";
import { PendingReplyEditingContext } from "./PendingReplyEditingContext";
import { setUiStylePrefs } from "../../utils/uiStyle";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { reason?: string; choice?: string }) =>
      values?.reason
        ? `${key}: ${values.reason}`
        : values?.choice
          ? `${key}: ${values.choice}`
          : key,
  }),
}));

type Props = Omit<ComponentProps<typeof PendingReplyCard>, "draft" | "onDraftChange"> & {
  show?: boolean;
  onEditingChange?: (editorId: string, focused: boolean) => void;
};

const question = {
  options: [
    { id: "first", label: "First", consequence: "Fast" },
    { id: "second", label: "Second", consequence: "More work" },
  ],
  recommended: { option_id: "first", reason: "It is ready" },
};

function Harness({ show = true, onEditingChange, ...props }: Props) {
  const [draft, setDraft] = useState<ReplyDraft>({ selection: null, text: "" });
  return (
    <PendingReplyEditingContext.Provider value={onEditingChange}>
      {show ? <PendingReplyCard {...props} draft={draft} onDraftChange={setDraft} /> : null}
    </PendingReplyEditingContext.Provider>
  );
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const render = (props: Props) => act(async () => root.render(<Harness {...props} />));
const submit = () => host.querySelector<HTMLButtonElement>('button[type="submit"]')!;
const radios = () => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
const changeText = async (value: string) => {
  const input = host.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
      input,
      value,
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
};

it("shows a recommendation without selecting it and submits only the chosen option", async () => {
  const onSubmit = vi.fn();
  await render({ question, state: "pending", onSubmit });
  expect(host.textContent).toContain("pendingReply.recommended");
  expect(host.textContent).toContain("It is ready");
  expect(radios().every((radio) => !radio.checked)).toBe(true);
  expect(submit().disabled).toBe(true);

  await act(async () => radios()[1]!.click());
  expect(submit().disabled).toBe(false);
  await act(async () => submit().click());
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({ kind: "option", id: "second" });
});

it("keeps Other text through row unmount and submits it without trimming", async () => {
  const onSubmit = vi.fn();
  const props = { question, state: "pending" as const, onSubmit };
  await render(props);
  await act(async () => radios()[2]!.click());
  await changeText("  自分の回答\n次の行  ");
  await render({ ...props, show: false });
  await render(props);
  expect(radios()[2]!.checked).toBe(true);
  expect(host.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe("  自分の回答\n次の行  ");
  await act(async () => submit().click());
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({
    kind: "other",
    text: "  自分の回答\n次の行  ",
  });
});

it("supports a plain request and refuses a blank answer", async () => {
  const onSubmit = vi.fn();
  await render({ state: "pending", onSubmit });
  expect(radios()).toHaveLength(0);
  expect(submit().disabled).toBe(true);
  await changeText(" \n ");
  expect(submit().disabled).toBe(true);
  await changeText("\u0085");
  expect(submit().disabled).toBe(true);
  await changeText("\ufeff");
  expect(submit().disabled).toBe(false);
  await changeText("Reply");
  await act(async () => submit().click());
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({ kind: "other", text: "Reply" });
});

it("counts Unicode scalars without truncating an answer that exceeds the limit", async () => {
  const onSubmit = vi.fn();
  await render({ state: "pending", onSubmit });
  const atLimit = "🌸".repeat(4000);
  await changeText(atLimit);
  expect(submit().disabled).toBe(false);
  await changeText(`${atLimit}🌸`);
  expect(submit().disabled).toBe(true);
  expect(host.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe(`${atLimit}🌸`);
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("pendingReply.tooLong");
  expect(onSubmit).not.toHaveBeenCalled();
});

it("does not offer a second submission while sending or after closure", async () => {
  const onSubmit = vi.fn();
  await render({ question, state: "pending", onSubmit });
  await act(async () => radios()[0]!.click());
  await render({ question, state: "pending", sending: true, onSubmit });
  expect(submit().disabled).toBe(true);
  expect(radios().every((radio) => radio.disabled)).toBe(true);

  for (const state of ["answered", "withdrawn", "unavailable", "unknown", "unsupported"] as const) {
    await render({ question, state, onSubmit });
    expect(host.querySelector('button[type="submit"]')).toBeNull();
    expect(host.querySelector('[role="status"]')?.textContent).toBe(`pendingReply.${state}`);
  }
  await render({ question, state: "pending", readOnly: true, onSubmit });
  expect(host.textContent).toBe("");
  expect(onSubmit).not.toHaveBeenCalled();
});

it("releases keyboard focus when a virtual row disappears or the request closes", async () => {
  const onSubmit = vi.fn();
  const onEditingChange = vi.fn();
  const props = { state: "pending" as const, onSubmit, onEditingChange };
  await render(props);
  await act(async () => host.querySelector<HTMLTextAreaElement>("textarea")!.focus());
  expect(onEditingChange).toHaveBeenCalledTimes(1);
  const editorId = onEditingChange.mock.calls[0]![0];
  expect(onEditingChange).toHaveBeenLastCalledWith(editorId, true);
  await render({ ...props, show: false });
  expect(onEditingChange).toHaveBeenLastCalledWith(editorId, false);

  await render(props);
  await act(async () => host.querySelector<HTMLTextAreaElement>("textarea")!.focus());
  const nextEditorId = onEditingChange.mock.calls.at(-1)![0];
  expect(onEditingChange).toHaveBeenLastCalledWith(nextEditorId, true);
  await render({ ...props, state: "withdrawn" });
  expect(onEditingChange).toHaveBeenLastCalledWith(nextEditorId, false);
});

it("keeps an in-progress draft visible when another person closes the request", async () => {
  const onSubmit = vi.fn();
  await render({ state: "pending", onSubmit, showRetainedDraft: true });
  await changeText("  My draft\nsecond line  ");
  await render({ state: "answered", onSubmit, showRetainedDraft: true });
  expect(host.textContent).toContain("pendingReply.retainedDraft");
  expect(host.textContent).toContain("  My draft\nsecond line  ");
  expect(host.querySelector('button[type="submit"]')).toBeNull();
});

it("keeps the focused editor usable while a fresh status is being checked", async () => {
  const onSubmit = vi.fn();
  await render({ state: "pending", onSubmit });
  await changeText("draft");
  const editor = host.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => editor.focus());
  await render({ state: "pending", checking: true, onSubmit });
  expect(document.activeElement).toBe(editor);
  expect(editor.disabled).toBe(false);
  expect(editor.value).toBe("draft");
  expect(submit().disabled).toBe(true);
  await render({ state: "pending", checking: false, onSubmit });
  expect(submit().disabled).toBe(false);
});

it("offers the existing chat reply for an unsupported source only when available", async () => {
  const onSubmit = vi.fn();
  const onUseComposer = vi.fn();
  await render({ state: "unsupported", onSubmit, onUseComposer });
  const button = host.querySelector<HTMLButtonElement>("button")!;
  expect(button.textContent).toBe("pendingReply.openComposer");
  await act(async () => button.click());
  expect(onUseComposer).toHaveBeenCalledOnce();
  expect(onSubmit).not.toHaveBeenCalled();
  await render({ state: "unsupported", onSubmit });
  expect(host.querySelector("button")).toBeNull();
});

it("adds the title block, in the product's existing words, only in the LCD preview", async () => {
  const onSubmit = vi.fn();
  await render({ question, state: "pending", onSubmit });
  expect(host.querySelector(".lcd-ask-title")).toBeNull();
  try {
    await act(async () => setUiStylePrefs({ style: "lcd" }));
    expect(host.querySelector("form .lcd-ask-title")?.textContent).toBe("filterWaitingForYou");
    await render({ question, state: "pending", sending: true, onSubmit });
    // The button says it is sending; the title keeps its words instead of repeating that.
    expect(host.querySelector(".lcd-ask-title")?.textContent).toBe("filterWaitingForYou");
    expect(
      [...host.querySelectorAll("button")].filter((b) => b.textContent === "pendingReply.sending"),
    ).toHaveLength(1);
    await render({ question, state: "answered", onSubmit });
    expect(host.querySelector(".lcd-ask-title")).toBeNull();
    expect(host.querySelector("[data-pending-reply-closed]")?.textContent).toContain(
      "pendingReply.answered",
    );
  } finally {
    await act(async () => setUiStylePrefs({ style: "classic" }));
  }
});

it("keeps the LCD action in the last row, enabled while the choices are locked", async () => {
  const onRetry = vi.fn();
  try {
    await act(async () => setUiStylePrefs({ style: "lcd" }));
    await render({ question, state: "pending", onSubmit: vi.fn() });
    // "Other" and the action share one row, which wraps instead of overlapping.
    const row = host.querySelector(".lcd-ask-other");
    expect(row?.querySelector('input[type="radio"]')).not.toBeNull();
    expect(row?.querySelector('.lcd-ask-action button[type="submit"]')).not.toBeNull();
    expect(host.querySelector("legend")?.className).toBe("sr-only");

    await act(async () => radios()[0]!.click());
    await render({ question, state: "pending", uncertain: true, onSubmit: vi.fn(), onRetry });
    const retry = host.querySelector<HTMLButtonElement>(".lcd-ask-other .lcd-ask-action button")!;
    expect(retry.textContent).toBe("pendingReply.retrySame");
    expect(radios().every((radio) => radio.disabled)).toBe(true);
    expect(retry.disabled).toBe(false);
    await act(async () => retry.click());
    expect(onRetry).toHaveBeenCalledOnce();

    await render({ question, state: "pending", onSubmit: vi.fn() });
    await act(async () => radios()[2]!.click());
    expect(host.querySelector(".lcd-ask-answer textarea")).not.toBeNull();
    expect(host.querySelector('.lcd-ask-answer button[type="submit"]')).not.toBeNull();
    expect(host.querySelector(".lcd-ask-other")).toBeNull();
  } finally {
    await act(async () => setUiStylePrefs({ style: "classic" }));
  }
});

it("names only the chosen letter when LCD already lists the options above", async () => {
  const answered = { id: "second", label: "Second" };
  await render({ question, state: "answered", answeredOption: answered, onSubmit: vi.fn() });
  const closed = () => host.querySelector("[data-pending-reply-closed]")?.textContent;
  expect(closed()).toBe("pendingReply.answeredChoice: second. Second");
  try {
    await act(async () => setUiStylePrefs({ style: "lcd" }));
    expect(closed()).toBe("pendingReply.answeredChoice: second");
  } finally {
    await act(async () => setUiStylePrefs({ style: "classic" }));
  }
});

it("grows the answer box with its text and fits it again when the box gets narrower", async () => {
  const observers: Array<() => void> = [];
  class ResizeObserverStub {
    constructor(callback: () => void) {
      observers.push(callback);
    }
    observe() {}
    disconnect() {}
  }
  vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  let natural = 60;
  let width = 300;
  const scrollHeight = vi
    .spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get")
    .mockImplementation(() => natural);
  const clientWidth = vi
    .spyOn(HTMLTextAreaElement.prototype, "clientWidth", "get")
    .mockImplementation(() => width);
  try {
    await render({ question: { options: [] }, state: "pending", onSubmit: vi.fn() });
    const box = host.querySelector<HTMLTextAreaElement>("textarea")!;
    expect(box.style.height).toBe("60px");
    natural = 140;
    await changeText("a long answer that wraps onto several lines");
    expect(box.style.height).toBe("140px");
    // The same text wraps onto more lines in a narrower box (a turned phone).
    natural = 200;
    width = 240;
    await act(async () => observers.forEach((notify) => notify()));
    expect(box.style.height).toBe("200px");
    // Its own height change does not count as a width change.
    natural = 260;
    await act(async () => observers.forEach((notify) => notify()));
    expect(box.style.height).toBe("200px");
  } finally {
    scrollHeight.mockRestore();
    clientWidth.mockRestore();
    vi.unstubAllGlobals();
  }
});

it("keeps the LCD send key's words as its name and its own icon while sending starts", async () => {
  vi.useFakeTimers();
  try {
    await act(async () => setUiStylePrefs({ style: "lcd" }));
    await render({ question: { options: [] }, state: "pending", onSubmit: vi.fn() });
    const key = () => host.querySelector<HTMLButtonElement>("[data-ask-send]")!;
    expect(key().getAttribute("aria-label")).toBe("pendingReply.send");
    expect(key().getAttribute("title")).toBe("pendingReply.send");
    expect(key().querySelector("[data-ask-send-label]")?.textContent).toBe("pendingReply.send");
    expect(key().querySelector("[data-ask-send-mark] svg")).not.toBeNull();

    await render({ question: { options: [] }, state: "pending", sending: true, onSubmit: vi.fn() });
    expect(key().getAttribute("aria-label")).toBe("pendingReply.sending");
    expect(key().disabled).toBe(true);
    // A quick send never blinks to an empty key: the send mark stays for 250 ms.
    expect(key().querySelector("[data-ask-send-mark] .lcd-spinner")).toBeNull();
    await act(async () => vi.advanceTimersByTime(250));
    expect(key().querySelector("[data-ask-send-mark] .lcd-spinner[data-shown]")).not.toBeNull();
  } finally {
    vi.useRealTimers();
    await act(async () => setUiStylePrefs({ style: "classic" }));
  }
});
