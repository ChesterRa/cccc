// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useMobileReadingChrome } from "./useMobileReadingChrome";

function Harness({
  enabled = true,
  onReachLatest,
  onAction,
}: {
  enabled?: boolean;
  onReachLatest?: () => void;
  onAction?: () => void;
}) {
  const scroll = useRef<HTMLDivElement>(null);
  const reading = useMobileReadingChrome(enabled, "group", scroll, undefined, onReachLatest);
  return (
    <section
      data-hidden={reading.hidden}
      onTouchStartCapture={reading.onTouchStartCapture}
      onTouchEndCapture={reading.onTouchEndCapture}
      onTouchCancelCapture={reading.onTouchCancelCapture}
      onScrollCapture={reading.onScrollCapture}
      onClickCapture={reading.onClickCapture}
      onFocusCapture={reading.onFocusCapture}
    >
      <div ref={scroll} role="log">
        <div>
          message <button onClick={onAction}>action</button>
        </div>
        <div data-pending-reply>
          <label htmlFor="reply-option">choice</label>
          <input id="reply-option" type="radio" />
          <textarea aria-label="answer" />
        </div>
      </div>
    </section>
  );
}

function touch(element: Element, type: string, y: number) {
  const event = new Event(type, { bubbles: true });
  Object.defineProperty(event, type === "touchstart" ? "touches" : "changedTouches", {
    value: [{ clientY: y }],
  });
  element.dispatchEvent(event);
}

afterEach(() => vi.useRealTimers());

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
      scrollTo(330);
      touch(message, "touchend", 460);
    });
    expect(hidden()).toBe(true);
    await act(async () => vi.advanceTimersByTime(1000));
    expect(hidden()).toBe(true);
    await act(async () => {
      touch(message, "touchstart", 460);
      scrollTo(315);
      touch(message, "touchend", 490);
    });
    expect(hidden()).toBe(false);
    await act(async () => {
      touch(message, "touchstart", 500);
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
      log.scrollTop = 370;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => editor.focus());
    expect(section.getAttribute("data-hidden")).toBe("false");
    await act(async () => {
      touch(message, "touchstart", 500);
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
      log.scrollTop = 330;
      log.dispatchEvent(new Event("scroll"));
      touch(message, "touchend", 460);
    });
    expect(section.getAttribute("data-hidden")).toBe("true");
    await act(async () => touch(button, "touchstart", 400));
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
