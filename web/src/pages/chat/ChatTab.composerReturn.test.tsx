// @vitest-environment happy-dom
import { act, createRef, type ReactNode, type RefObject } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { PresentationMessageRef, VoiceDocumentMessageRef } from "../../types";
import type { useChatTab } from "../../hooks/useChatTab";

const captured = vi.hoisted(() => ({
  onAttachPath: null as ((path: string) => void) | null,
  onQuoteInChat: null as
    | ((payload: { slotId: string; ref?: PresentationMessageRef | null }) => void)
    | null,
  onQuoteVoiceDocumentRef: null as ((ref: VoiceDocumentMessageRef) => void) | null,
  setComposerText: vi.fn(),
  setQuotedPresentationRef: vi.fn(),
  setQuotedVoiceDocumentRef: vi.fn(),
  setDestGroupId: vi.fn(),
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../components/VirtualMessageList", () => ({ VirtualMessageList: () => null }));
vi.mock("../../components/messageBubble/AttachmentLightbox", () => ({
  AttachmentLightbox: () => null,
}));
vi.mock("./GroupWorkArea", () => ({
  GroupWorkArea: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("./RuntimeDock", () => ({ RuntimeDock: () => null }));
vi.mock("./ChatFilterTabs", () => ({ ChatFilterTabs: () => null }));
vi.mock("./ChatSenderFilter", () => ({ ChatSenderFilter: () => null }));
vi.mock("./ComposerFailedSends", () => ({ ComposerFailedSends: () => null }));
vi.mock("../../components/layout/ResizableSidePanel", () => ({
  ResizableSidePanel: ({
    children,
  }: {
    children: (args: { compact: boolean; toggleCompact: () => void }) => ReactNode;
  }) => <div>{children({ compact: false, toggleCompact: () => undefined })}</div>,
}));
vi.mock("../../components/resourcePanel/ResourcePanel", () => ({
  ResourcePanel: ({
    onAttachPath,
    presentation,
  }: {
    onAttachPath?: (path: string) => void;
    presentation?: {
      handlers?: {
        onQuoteInChat?: (payload: { slotId: string; ref?: PresentationMessageRef | null }) => void;
      };
    };
  }) => {
    captured.onAttachPath = onAttachPath ?? null;
    captured.onQuoteInChat = presentation?.handlers?.onQuoteInChat ?? null;
    return null;
  },
}));
vi.mock("./ChatComposer", () => ({
  ChatComposer: ({
    composerRef,
    onQuoteVoiceDocumentRef,
  }: {
    composerRef: RefObject<HTMLTextAreaElement | null>;
    onQuoteVoiceDocumentRef?: (ref: VoiceDocumentMessageRef) => void;
  }) => {
    captured.onQuoteVoiceDocumentRef = onQuoteVoiceDocumentRef ?? null;
    return <textarea ref={composerRef} data-test-composer />;
  },
}));
type UseChatTabResult = ReturnType<typeof useChatTab>;

function createMockUseChatTabResult(): UseChatTabResult {
  return {
    chatMessages: [],
    suggestionSourceMessages: [],
    liveWorkEvents: [],
    hasAnyChatMessages: false,
    chatFilter: "all",
    setChatFilter: vi.fn(),
    chatSenderFilter: [],
    setChatSenderFilter: vi.fn(),
    senderHistoryError: "",
    retrySenderHistory: vi.fn(),
    pendingPageError: false,
    pendingDaemonUnsupported: false,
    retryPendingPage: vi.fn(),
    pendingActive: false,
    pinnedActive: false,
    pinnedError: false,
    pinnedUnavailable: [],
    retryPinned: vi.fn(),
    pinnedTabOffered: false,
    taskView: null,
    taskViewLoading: false,
    taskViewError: false,
    taskViewTruncated: false,
    taskViewContextIncomplete: false,
    retryTaskView: vi.fn(),
    toggleMessagePin: undefined,
    viewMessageContext: vi.fn(),
    chatViewKey: "all",
    chatWindowProps: null,
    chatInitialScrollTargetId: undefined,
    chatInitialScrollAnchorId: undefined,
    chatInitialScrollAnchorOffsetPx: undefined,
    chatInitialScrollOffsetPx: undefined,
    chatInitialScrollHighlight: false,
    chatHighlightEventId: undefined,
    liveJumpActive: false,
    inChatWindow: false,
    isLoadingHistory: false,
    hasMoreHistory: false,
    loadMoreHistory: undefined,
    chatEmptyState: "ready",
    busy: "",
    sendInFlight: false,
    uploadInFlight: false,
    stopUploadWaiting: vi.fn(),
    showScrollButton: false,
    chatUnreadCount: 0,
    sendScrollRequest: null,
    consumeSendScrollRequest: vi.fn(),
    showSetupCard: false,
    needsScope: false,
    needsActors: false,
    needsStart: false,
    hasForeman: false,
    setComposerText: captured.setComposerText,
    composerGroupMentionTokens: [],
    setComposerGroupMentionTokens: vi.fn(),
    composerAgentMentionTokens: [],
    setComposerAgentMentionTokens: vi.fn(),
    composerFiles: [],
    setComposerFiles: vi.fn(),
    removeComposerFile: vi.fn(),
    replyTarget: null,
    quotedPresentationRef: null,
    quotedVoiceDocumentRef: null,
    cancelReply: vi.fn(),
    clearQuotedPresentationRef: vi.fn(),
    setQuotedVoiceDocumentRef: captured.setQuotedVoiceDocumentRef,
    clearQuotedVoiceDocumentRef: vi.fn(),
    toTokens: [],
    unavailableRecipients: [],
    toggleRecipient: vi.fn(),
    clearRecipients: vi.fn(),
    messageMode: "send",
    setMessageMode: vi.fn(),
    destGroupId: "g1",
    setDestGroupId: captured.setDestGroupId,
    composerGroupSettled: true,
    composerRouteGroups: [],
    mentionSuggestions: [],
    connectMentionStatus: undefined,
    agentStates: [],
    taskById: new Map(),
    sendMessage: vi.fn(),
    slashCommands: [],
    copyMessageLink: vi.fn(),
    startReply: vi.fn(),
    showRecipients: vi.fn(),
    relayMessage: vi.fn(),
    openSourceMessage: vi.fn(),
    exitChatWindow: vi.fn(),
    returnToLatest: vi.fn(),
    handleScrollButtonClick: vi.fn(),
    handleScrollChange: vi.fn(),
    onContextReady: vi.fn(),
    onReachedContextEnd: vi.fn(),
    onLoadNewerContext: vi.fn(),
    handleScrollSnapshot: vi.fn(),
    addAgent: vi.fn(),
  };
}

vi.mock("../../hooks/useChatTab", () => ({ useChatTab: () => createMockUseChatTabResult() }));
vi.mock("../../components/workspace/useWorkspaceFiles", () => ({
  useWorkspaceFiles: () => ({
    mode: "files",
    setMode: vi.fn(),
    scopeKey: "s1",
    scopeUrl: "/repo",
    openFile: vi.fn(),
    changes: { selection: null },
  }),
}));
vi.mock("../../features/pendingReplies/useWaitingFilterIndicator", () => ({
  useWaitingFilterIndicator: () => null,
}));
vi.mock("../../features/pendingReplies/useLivePendingStatuses", () => ({
  useLivePendingStatuses: () => ({}),
}));
vi.mock("../../features/pushNotifications/useNotificationPromptOffer", () => ({
  useNotificationPromptOffer: () => ({ visible: false, dismiss: vi.fn() }),
}));
vi.mock("../../features/pins/pinStore", () => ({ usePinnedIds: () => new Set<string>() }));
vi.mock("../../features/quickReplies/templateStore", () => ({
  usePreloadQuickReplyTemplates: () => undefined,
}));
vi.mock("../../hooks/useShortTouchViewport", () => ({ useShortTouchViewport: () => false }));
vi.mock("../../hooks/useMobileReadingChrome", () => ({
  useMobileReadingChrome: () => ({
    hidden: false,
    overlay: false,
    onFocusCapture: vi.fn(),
    onScroll: vi.fn(),
  }),
}));
vi.mock("../../hooks/useKeepInlineReplyVisible", () => ({
  useKeepInlineReplyVisible: () => undefined,
}));
vi.mock("./useRuntimeDockWorkCards", () => ({ useRuntimeDockWorkCards: () => [] }));
vi.mock("./useFloatingDockReserve", () => ({ useFloatingDockReserve: () => 0 }));
vi.mock("./useWorkspaceTabChoice", () => ({ useWorkspaceTabChoice: () => vi.fn() }));
vi.mock("../../hooks/useSidePanelSelection", () => ({
  useSidePanelSelection: () => ({ activeSidePanel: null, selectSidePanel: vi.fn() }),
}));

import { ChatTab } from "./ChatTab";
import { useComposerStore, useGroupStore, useUIStore } from "../../stores";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

describe("ChatTab returns from terminals view when external actions populate the composer", () => {
  let host: HTMLDivElement;
  let root: Root;
  let composerRef: RefObject<HTMLTextAreaElement | null>;

  beforeEach(async () => {
    vi.useFakeTimers();
    captured.onAttachPath = null;
    captured.onQuoteInChat = null;
    captured.onQuoteVoiceDocumentRef = null;
    captured.setComposerText.mockReset();
    captured.setQuotedPresentationRef.mockReset();
    captured.setQuotedVoiceDocumentRef.mockReset();
    captured.setDestGroupId.mockReset();

    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    composerRef = createRef<HTMLTextAreaElement>();

    useGroupStore.setState({
      groupDoc: {
        group_id: "g1",
        title: "Group One",
        scopes: [{ scope_key: "s1", url: "/repo" }],
        active_scope_key: "s1",
      },
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
    useResourcePanelStore.getState().setOpen("g1", true);

    await act(async () => {
      root.render(
        <ChatTab
          isDark={false}
          isSmallScreen={false}
          selectedGroupId="g1"
          selectedGroupRunning={true}
          selectedGroupActorsHydrating={false}
          selectedGroupActorStatusProvisional={false}
          groupLabelById={{ g1: "Group One" }}
          actors={[]}
          runtimeActors={[]}
          renderRuntimeActor={() => <div />}
          recipientActors={[]}
          scrollRef={{ current: null }}
          composerRef={composerRef}
          fileInputRef={{ current: null }}
          appendComposerFiles={vi.fn()}
          onStartGroup={vi.fn()}
          onOpenRuntimeActor={vi.fn()}
          showMentionMenu={false}
          setShowMentionMenu={vi.fn()}
          mentionSelectedIndex={0}
          setMentionSelectedIndex={vi.fn()}
          setMentionFilter={vi.fn()}
          setMentionKind={vi.fn()}
          setMentionActorScope={vi.fn()}
          setMentionTargetGroupId={vi.fn()}
        />,
      );
      await Promise.resolve();
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useUIStore.getState().setGroupWorkView("g1", "messages");
    useResourcePanelStore.getState().setOpen("g1", false);
    useComposerStore.setState({
      destGroupId: "g1",
      quotedPresentationRef: null,
      quotedVoiceDocumentRef: null,
    });
    vi.useRealTimers();
  });

  it("switches to messages and focuses the composer when attaching a workspace path from terminals view", async () => {
    const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
    expect(slot().hidden).toBe(true);
    expect(captured.onAttachPath).not.toBeNull();

    await act(async () => {
      captured.onAttachPath!("src/app.ts");
    });
    await act(async () => {
      vi.runAllTimers();
    });

    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("messages");
    expect(slot().hidden).toBe(false);
    expect(slot().hasAttribute("inert")).toBe(false);
    expect(document.activeElement).toBe(composerRef.current);
    expect(captured.setComposerText).toHaveBeenCalledOnce();
  });

  it("switches to messages and focuses the composer when quoting a presentation slot from terminals view", async () => {
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
    expect(document.activeElement).toBe(composerRef.current);
    expect(useComposerStore.getState().quotedPresentationRef?.slot_id).toBe("slot-1");
  });

  it("switches to messages and focuses the composer when quoting a voice document from terminals view", async () => {
    const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
    expect(slot().hidden).toBe(true);
    expect(captured.onQuoteVoiceDocumentRef).not.toBeNull();

    await act(async () => {
      captured.onQuoteVoiceDocumentRef!({
        kind: "voice_document_ref",
        group_id: "g1",
        document_path: "voice/notes.md",
        title: "Notes",
      });
    });
    await act(async () => {
      vi.runAllTimers();
    });

    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("messages");
    expect(slot().hidden).toBe(false);
    expect(document.activeElement).toBe(composerRef.current);
    expect(captured.setQuotedVoiceDocumentRef).toHaveBeenCalledOnce();
  });
});
