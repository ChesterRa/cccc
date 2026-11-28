// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  CHAT_MESSAGE_VIEW_CLASS,
  CHAT_VIEW_SETTLED_ATTRIBUTE,
  CHAT_VIEW_SETTLE_LIMIT_MS,
  runChatViewTransition,
} from "./chatViewTransition";

type Update = () => void | Promise<void>;

describe("runChatViewTransition", () => {
  const messageView = (settled: boolean) => {
    const view = document.createElement("div");
    view.className = CHAT_MESSAGE_VIEW_CLASS;
    view.setAttribute(CHAT_VIEW_SETTLED_ATTRIBUTE, String(settled));
    // happy-dom lays nothing out; the shown list is the one that reports a box.
    view.getClientRects = () => [{}] as unknown as DOMRectList;
    document.body.append(view);
    return view;
  };
  const nameOf = (view: HTMLElement) => view.style.getPropertyValue("view-transition-name");
  /** A browser that runs the update, then finishes the transition when asked to. */
  const browser = () => {
    let finish = () => {};
    const finished = new Promise<void>((resolve) => (finish = resolve));
    let updating: Promise<void> = Promise.resolve();
    const start = vi.fn((update: Update) => {
      updating = Promise.resolve(update());
      return { finished };
    });
    Object.assign(document, { startViewTransition: start });
    return { start, finish, updating: () => updating };
  };

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
    Reflect.deleteProperty(document, "startViewTransition");
    vi.unstubAllGlobals();
  });

  it("just makes the change where the browser has no view transitions", () => {
    messageView(true);
    const update = vi.fn();

    runChatViewTransition(update);

    expect(update).toHaveBeenCalledOnce();
  });

  it("just makes the change for a user who asked for reduced motion", () => {
    messageView(true);
    const { start } = browser();
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const update = vi.fn();

    runChatViewTransition(update);

    expect(update).toHaveBeenCalledOnce();
    expect(start).not.toHaveBeenCalled();
  });

  it("just makes the change when no message list is on screen", () => {
    const { start } = browser();
    const update = vi.fn();

    runChatViewTransition(update);

    expect(update).toHaveBeenCalledOnce();
    expect(start).not.toHaveBeenCalled();
  });

  it("animates the leaving list out and the list that replaces it in, then lets go of both", async () => {
    const leaving = messageView(true);
    const { finish, updating } = browser();
    let arriving!: HTMLElement;

    runChatViewTransition(() => {
      // A different view mounts a different list element.
      leaving.remove();
      arriving = messageView(true);
    });

    // The old picture was taken of the leaving list; the new one must be of the arriving list.
    expect(nameOf(leaving)).toBe("");
    expect(nameOf(arriving)).toBe("chat-messages");
    await vi.advanceTimersByTimeAsync(100);
    await updating();
    finish();
    await vi.advanceTimersByTimeAsync(0);
    expect(nameOf(arriving)).toBe("");
  });

  it("keeps the old messages up until the new view has read its own", async () => {
    const view = messageView(false);
    const { updating } = browser();
    let shown = false;
    runChatViewTransition(() => undefined);
    void updating().then(() => (shown = true));

    await vi.advanceTimersByTimeAsync(120);
    expect(shown).toBe(false);

    view.setAttribute(CHAT_VIEW_SETTLED_ATTRIBUTE, "true");
    await vi.advanceTimersByTimeAsync(80);
    expect(shown).toBe(true);
  });

  it("stops waiting after a short limit, so a slow read cannot freeze the conversation", async () => {
    messageView(false);
    const { updating } = browser();
    let shown = false;
    runChatViewTransition(() => undefined);
    void updating().then(() => (shown = true));

    await vi.advanceTimersByTimeAsync(CHAT_VIEW_SETTLE_LIMIT_MS + 100);

    expect(shown).toBe(true);
  });

  it("still makes the change when the browser refuses the transition", () => {
    const view = messageView(true);
    Object.assign(document, {
      startViewTransition: () => {
        throw new Error("fixture refusal");
      },
    });
    const update = vi.fn();

    runChatViewTransition(update);

    expect(update).toHaveBeenCalledOnce();
    expect(nameOf(view)).toBe("");
  });
});
