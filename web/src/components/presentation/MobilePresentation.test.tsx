// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupPresentation } from "../../types";
import { MobilePresentationTrigger } from "./MobilePresentationTrigger";
import { MobilePresentationSurface } from "./MobilePresentationSurface";
import { PresentationRail } from "./PresentationRail";
import {
  resolveMobilePresentationHighlight,
  shouldShowMobilePresentationTrigger,
} from "./mobilePresentationModel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
    i18n: { language: "en" },
  }),
}));

const presentation: GroupPresentation = {
  v: 1,
  highlight_slot_id: "slot-1",
  slots: [
    {
      slot_id: "slot-1",
      index: 1,
      card: {
        slot_id: "slot-1",
        title: "Mobile preview",
        card_type: "web_preview",
        published_by: "agent",
        published_at: "2026-08-27T00:00:00Z",
        content: { mode: "reference", url: "http://127.0.0.1:4173/zh" },
      },
    },
    { slot_id: "slot-2", index: 2 },
  ],
};

describe("mobile presentation entry", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("stays discoverable on every selected mobile chat, including empty presentations", () => {
    expect(
      shouldShowMobilePresentationTrigger({
        isSmallScreen: true,
        hasChatWindow: false,
        groupId: "g_demo",
      }),
    ).toBe(true);
    expect(
      shouldShowMobilePresentationTrigger({
        isSmallScreen: false,
        hasChatWindow: false,
        groupId: "g_demo",
      }),
    ).toBe(false);
  });

  it("shows the highlighted slot and remains pointer-interactive", async () => {
    const onOpen = vi.fn();
    await act(async () => {
      root.render(
        <MobilePresentationTrigger
          presentation={presentation}
          attentionSlots={{ "slot-1": true }}
          isDark={false}
          onOpen={onOpen}
        />,
      );
    });

    const button = host.querySelector("button");
    expect(resolveMobilePresentationHighlight(presentation)?.slot_id).toBe("slot-1");
    expect(button?.textContent).toContain("Presentation");
    expect(button?.textContent).toContain("1");
    expect(button?.className).toContain("pointer-events-auto");
    expect(button?.dataset.mobilePresentationTrigger).toBe("true");

    await act(async () => button?.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});

describe("mobile presentation surface", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("uses a safe-area full-screen portal and closes with Escape", async () => {
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <MobilePresentationSurface isOpen isDark={false} label="Presentation" onClose={onClose}>
          <button type="button">First</button>
          <button type="button">Last</button>
        </MobilePresentationSurface>,
      );
    });

    const surface = document.querySelector<HTMLElement>("[data-mobile-presentation-surface]");
    expect(surface?.getAttribute("role")).toBe("dialog");
    expect(surface?.className).toContain("fixed inset-0");
    expect(surface?.className).toContain("safe-area-inset-top");
    expect(surface?.textContent).toContain("First");

    const buttons = surface?.querySelectorAll<HTMLButtonElement>("button") || [];
    const first = buttons[0];
    const last = buttons[1];
    last.focus();
    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }),
      ),
    );
    expect(document.activeElement).toBe(first);

    first.focus();
    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(document.activeElement).toBe(last);

    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("renders a clear back control and a one-column phone slot list", async () => {
    await act(async () => {
      root.render(
        <PresentationRail
          mode="panel"
          presentation={presentation}
          isDark={false}
          isOpen
          attentionSlots={{}}
          onOpenChange={() => undefined}
          onOpenSlot={() => undefined}
        />,
      );
    });

    expect(host.querySelector("[data-mobile-presentation-close]")).not.toBeNull();
    expect(host.querySelector(".grid")?.className).toContain("grid-cols-1");
    expect(host.querySelector(".grid")?.className).toContain("min-[420px]:grid-cols-2");
  });
});
