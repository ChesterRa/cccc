// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useMobileReadingChrome } from "./useMobileReadingChrome";

function Harness({
  enabled = true,
  onReachLatest,
  onAction,
  composerDraft = "",
}: {
  enabled?: boolean;
  onReachLatest?: () => void;
  onAction?: () => void;
  composerDraft?: string;
}) {
  const scroll = useRef<HTMLDivElement>(null);
  const reading = useMobileReadingChrome(enabled, "group", scroll, undefined, onReachLatest);
  return (
    <section
      data-hidden={reading.hidden}
      onTouchStartCapture={reading.onTouchStartCapture}
      onTouchMoveCapture={reading.onTouchMoveCapture}
      onTouchEndCapture={reading.onTouchEndCapture}
      onTouchCancelCapture={reading.onTouchCancelCapture}
      onCompositionStartCapture={reading.onCompositionStartCapture}
      onCompositionEndCapture={reading.onCompositionEndCapture}
      onScrollCapture={reading.onScrollCapture}
      onClickCapture={reading.onClickCapture}
      onFocusCapture={reading.onFocusCapture}
    >
      <div ref={scroll} role="log">
        <div>
          message <a href="#fixture">link</a> <button onClick={onAction}>action</button>
        </div>
        <div data-pending-reply>
          <label htmlFor="reply-option">choice</label>
          <input id="reply-option" type="radio" />
          <textarea aria-label="answer" />
        </div>
      </div>
      <div data-message-filters>
        <button>filter</button>
      </div>
      <footer>
        <div data-composer-input>
          <textarea defaultValue={composerDraft} aria-label="composer" />
        </div>
      </footer>
    </section>
  );
}

function touch(element: Element, type: string, y: number) {
  const event = new Event(type, { bubbles: true });
  Object.defineProperty(event, type === "touchend" ? "changedTouches" : "touches", {
    value: [{ clientY: y }],
  });
  element.dispatchEvent(event);
}

afterEach(() => vi.useRealTimers());

it.each(["a", "button"])("tracks a touch scroll that starts on a message %s", async (tag) => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const target = log.querySelector(tag)!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(target, "touchstart", 500);
      touch(target, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(target, "touchend", 460);
    });
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("true");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("lets a touch scroll leave a previously focused filter and hides the chrome", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const filter = host.querySelector<HTMLButtonElement>("[data-message-filters] button")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => filter.focus());
    expect(document.activeElement).toBe(filter);
    await act(async () => {
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("true");
    expect(document.activeElement).not.toBe(filter);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("does not mistake a programmatic scroll while touching an action for a drag", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const action = vi.fn();
  try {
    await act(async () => root.render(<Harness onAction={action} />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const button = log.querySelector<HTMLButtonElement>("button")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(button, "touchstart", 400);
      log.scrollTop = 350;
      log.dispatchEvent(new Event("scroll"));
      touch(button, "touchend", 400);
    });
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("false");
    await act(async () => button.click());
    expect(action).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps an open layer and its focused controls visible during a touch scroll", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  const menu = document.createElement("div");
  menu.setAttribute("role", "menu");
  menu.setAttribute("data-state", "open");
  document.body.append(host, menu);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const filter = host.querySelector<HTMLButtonElement>("[data-message-filters] button")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => filter.focus());
    await act(async () => {
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("false");
    expect(document.activeElement).toBe(filter);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    menu.remove();
  }
});

it.each([
  ["empty", "", 330, "true"],
  ["draft", "keep this draft", 270, "false"],
])(
  "ends %s composer focus after a touch scroll without losing text",
  async (_name, draft, top, hidden) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Harness composerDraft={draft} />));
      const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
      const editor = host.querySelector<HTMLTextAreaElement>("footer textarea")!;
      Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
      Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
      log.scrollTop = 300;
      await act(async () => editor.focus());
      await act(async () => {
        touch(log.firstElementChild!, "touchstart", 500);
        touch(log.firstElementChild!, "touchmove", top > 300 ? 460 : 540);
        log.scrollTop = top;
        log.dispatchEvent(new Event("scroll"));
        touch(log.firstElementChild!, "touchend", top > 300 ? 460 : 540);
      });
      expect(document.activeElement).not.toBe(editor);
      expect(editor.value).toBe(draft);
      expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe(hidden);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  },
);

it("does not blur the composer while an input method is composing", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness composerDraft="draft" />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const editor = host.querySelector<HTMLTextAreaElement>("footer textarea")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      editor.focus();
      editor.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(document.activeElement).toBe(editor);
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("false");
    await act(async () => {
      editor.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 360;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(document.activeElement).not.toBe(editor);
    expect(editor.value).toBe("draft");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("signals reaching the latest message without interrupting composition", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const latest = vi.fn();
  try {
    await act(async () => root.render(<Harness onReachLatest={latest} />));
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const editor = host.querySelector<HTMLTextAreaElement>("footer textarea")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("true");
    await act(async () => {
      editor.focus();
      editor.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      touch(log.firstElementChild!, "touchstart", 500);
      touch(log.firstElementChild!, "touchmove", 460);
      log.scrollTop = 1598;
      log.dispatchEvent(new Event("scroll"));
      touch(log.firstElementChild!, "touchend", 460);
    });
    expect(latest).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(editor);
    expect(host.querySelector("section")?.getAttribute("data-hidden")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("responds to touch scrolling without reacting to programmatic scrolls", async () => {
  vi.useFakeTimers();
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const section = host.querySelector("section")!;
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const message = log.firstElementChild!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    const hidden = () => section.getAttribute("data-hidden") === "true";
    const scrollTo = (top: number) => {
      log.scrollTop = top;
      log.dispatchEvent(new Event("scroll"));
    };

    await act(async () => scrollTo(300));
    expect(hidden()).toBe(false);
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      scrollTo(330);
      touch(message, "touchend", 460);
    });
    expect(hidden()).toBe(true);
    await act(async () => vi.advanceTimersByTime(1000));
    expect(hidden()).toBe(true);
    await act(async () => {
      touch(message, "touchstart", 460);
      touch(message, "touchmove", 490);
      scrollTo(315);
      touch(message, "touchend", 490);
    });
    expect(hidden()).toBe(false);
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      scrollTo(350);
      touch(message, "touchend", 460);
    });
    expect(hidden()).toBe(true);
    await act(async () => {
      touch(message, "touchstart", 400);
      touch(message, "touchend", 400);
    });
    expect(hidden()).toBe(false);
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      scrollTo(390);
      touch(message, "touchend", 460);
    });
    expect(hidden()).toBe(true);
    await act(async () => root.render(<Harness enabled={false} />));
    expect(hidden()).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps inline reply controls usable while the mobile chrome is hidden", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const section = host.querySelector("section")!;
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const message = log.firstElementChild!;
    const label = host.querySelector("label")!;
    const editor = host.querySelector<HTMLTextAreaElement>("textarea")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => {
      touch(label, "touchstart", 400);
      touch(label, "touchend", 400);
    });
    // The target must not move before its click is delivered.
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => label.click());
    expect(host.querySelector<HTMLInputElement>('input[type="radio"]')!.checked).toBe(true);
    expect(section.getAttribute("data-hidden")).toBe("false");

    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      log.scrollTop = 370;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => editor.focus());
    expect(section.getAttribute("data-hidden")).toBe("false");
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      log.scrollTop = 410;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("delivers a message action before restoring the controls", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const action = vi.fn();
  try {
    await act(async () => root.render(<Harness onAction={action} />));
    const section = host.querySelector("section")!;
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const message = log.firstElementChild!;
    const button = host.querySelector<HTMLButtonElement>("button")!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => touch(button, "touchstart", 400));
    // A new message or another programmatic update can take the list to the
    // end before the pending tap is delivered.
    await act(async () => {
      log.scrollTop = 1598;
      log.dispatchEvent(new Event("scroll"));
      touch(button, "touchend", 400);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => button.click());
    expect(action).toHaveBeenCalledOnce();
    expect(section.getAttribute("data-hidden")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("signals when restoring the controls would otherwise clip the latest message", async () => {
  vi.useFakeTimers();
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const latest = vi.fn();
  try {
    await act(async () => root.render(<Harness onReachLatest={latest} />));
    const section = host.querySelector("section")!;
    const log = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const message = log.firstElementChild!;
    Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(log, "clientHeight", { configurable: true, value: 400 });
    log.scrollTop = 300;
    await act(async () => {
      touch(message, "touchstart", 500);
      touch(message, "touchmove", 460);
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    // The return-to-latest button or a long inertial scroll may arrive after
    // the touch tracking window has ended.
    await act(async () => vi.advanceTimersByTime(700));
    await act(async () => {
      log.scrollTop = 1598;
      log.dispatchEvent(new Event("scroll"));
    });
    expect(latest).toHaveBeenCalledOnce();
    expect(section.getAttribute("data-hidden")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
