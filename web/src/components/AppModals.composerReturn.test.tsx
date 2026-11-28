// @vitest-environment happy-dom
import { act, createRef, type RefObject } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent, PresentationMessageRef } from "../types";

const captured = vi.hoisted(() => ({
  onQuoteInChat: null as
    | ((payload: { slotId: string; ref?: PresentationMessageRef | null }) => void)
    | null,
  onSearchReply: null as ((ev: LedgerEvent) => void) | null,
}));

vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("../services/api", () => ({
  fetchGroupPrompts: async () => ({ ok: true, result: { help: { content: "" } } }),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
}));
vi.mock("./modals/ActorConfigModal", () => ({ ActorConfigModal: () => null }));
vi.mock("./layout/MobileMenuSheet", () => ({ MobileMenuSheet: () => null }));
vi.mock("./modals/CreateGroupModal", () => ({ CreateGroupModal: () => null }));
vi.mock("./modals/GroupEditModal", () => ({ GroupEditModal: () => null }));
vi.mock("./modals/InboxModal", () => ({ InboxModal: () => null }));
vi.mock("./presentation/PresentationPinModal", () => ({ PresentationPinModal: () => null }));
vi.mock("./modals/RelayMessageModal", () => ({ RelayMessageModal: () => null }));
vi.mock("./modals/RecipientsModal", () => ({ RecipientsModal: () => null }));
vi.mock("./SearchModal", () => ({
  SearchModal: ({ onReply }: { onReply?: (ev: LedgerEvent) => void }) => {
    captured.onSearchReply = onReply ?? null;
    return null;
  },
}));
vi.mock("./presentation/PresentationViewerModal", () => ({
  PresentationViewerModal: ({
    onQuoteInChat,
  }: {
    onQuoteInChat?: (payload: { slotId: string; ref?: PresentationMessageRef | null }) => void;
  }) => {
    captured.onQuoteInChat = onQuoteInChat ?? null;
    return null;
  },
}));

import { AppModals } from "./AppModals";
import { WorkViewComposerSlot } from "../pages/chat/WorkViewComposerSlot";
import { useComposerStore, useGroupStore, useModalStore, useUIStore } from "../stores";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

describe("AppModals returns from terminals view when quoting or replying into the composer", () => {
  let host: HTMLDivElement;
  let root: Root;
  let composerRef: RefObject<HTMLTextAreaElement | null>;
  let onStartReply: ReturnType<typeof vi.fn<(ev: LedgerEvent) => void>>;

  beforeEach(async () => {
    vi.useFakeTimers();
    captured.onQuoteInChat = null;
    captured.onSearchReply = null;
    onStartReply = vi.fn<(ev: LedgerEvent) => void>();

    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    composerRef = createRef<HTMLTextAreaElement>();

    useGroupStore.setState({
      selectedGroupId: "g1",
      groupDoc: { group_id: "g1", title: "Group One" },
      groupPresentation: {
        v: 1,
        slots: [
          {
            slot_id: "slot-1",
            index: 1,
            card: {
              slot_id: "slot-1",
              title: "Architecture",
              card_type: "markdown",
              published_by: "user",
              published_at: "2026-11-27T00:00:00Z",
              content: { markdown: "# Plan" },
            },
          },
        ],
      },
    });
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    useModalStore.setState({
      presentationViewer: { groupId: "g1", slotId: "slot-1", surface: "modal" },
    });

    await act(async () => {
      root.render(
        <>
          <WorkViewComposerSlot groupId="g1">
            <textarea ref={composerRef} defaultValue="kept draft" data-test-composer />
          </WorkViewComposerSlot>
          <AppModals
            codexVoice={{} as React.ComponentProps<typeof AppModals>["codexVoice"]}
            isDark={false}
            theme="light"
            textScale={100}
            ccccHome="/fixture-only"
            composerRef={composerRef}
            onStartReply={onStartReply}
            onThemeChange={vi.fn()}
            onTextScaleChange={vi.fn()}
            onDeleteGroup={vi.fn()}
            fetchContext={vi.fn()}
            canManageGroups
          />
        </>,
      );
      await Promise.resolve();
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useUIStore.getState().setGroupWorkView("g1", "messages");
    useModalStore.setState({ presentationViewer: null });
    useComposerStore.setState({ quotedPresentationRef: null });
    vi.useRealTimers();
  });

  it("switches from terminals to messages, keeps the draft, and focuses the composer when quoting from the presentation modal", async () => {
    const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
    expect(slot().hidden).toBe(true);
    expect(captured.onQuoteInChat).not.toBeNull();

    await act(async () => {
      captured.onQuoteInChat!({ slotId: "slot-1" });
    });
    await act(async () => {
      vi.runAllTimers();
    });

    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("messages");
    expect(slot().hidden).toBe(false);
    expect(slot().hasAttribute("inert")).toBe(false);
    expect(composerRef.current?.value).toBe("kept draft");
    expect(document.activeElement).toBe(composerRef.current);
    expect(useComposerStore.getState().quotedPresentationRef?.slot_id).toBe("slot-1");
  });

  it("switches from terminals to messages, keeps the draft, and focuses the composer when replying from SearchModal", async () => {
    const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
    expect(slot().hidden).toBe(true);
    expect(captured.onSearchReply).not.toBeNull();

    const event: LedgerEvent = {
      id: "evt-1",
      kind: "chat.message",
      by: "peer-1",
      ts: "2026-11-27T00:00:00Z",
      data: { text: "Hello", to: ["user"] },
    };
    await act(async () => {
      captured.onSearchReply!(event);
    });
    await act(async () => {
      vi.runAllTimers();
    });

    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("messages");
    expect(slot().hidden).toBe(false);
    expect(slot().hasAttribute("inert")).toBe(false);
    expect(composerRef.current?.value).toBe("kept draft");
    expect(document.activeElement).toBe(composerRef.current);
    expect(onStartReply).toHaveBeenCalledWith(event);
  });
});
