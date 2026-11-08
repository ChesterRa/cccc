import type { MutableRefObject } from "react";
import type { ChatScrollSnapshot } from "../../stores/useUIStore";
import type {
  Actor,
  AgentState,
  LedgerEvent,
  PresentationMessageRef,
  Task,
  TaskMessageRef,
} from "../../types";
import type { WebModelDeliveryStatus } from "../../utils/webModelDeliveryStatus";
import type { ChatSendScrollRequest } from "../../utils/chatSendScrollRequest";

export interface VirtualMessageListProps {
  messages: LedgerEvent[];
  actors: Actor[];
  agentStates: AgentState[];
  taskById: Map<string, Task>;
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  webModelDeliveryStatusByEventId?: Record<string, WebModelDeliveryStatus>;
  viewKey?: string;
  followOnViewChangeKey?: string;
  initialScrollTargetId?: string;
  initialScrollAnchorId?: string;
  initialScrollAnchorOffsetPx?: number;
  initialScrollOffsetPx?: number;
  /** Mark the initial anchor briefly once the view opens on it: it stands in for a message the
   * reader did not see in this view. */
  highlightInitialScrollAnchor?: boolean;
  highlightEventId?: string;
  className?: string;
  topInsetPx?: number;
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onReply: (event: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  /** Pin or unpin a message; absent where pins are not offered. */
  onTogglePin?: (eventId: string, pinned: boolean) => void;
  onViewContext?: (eventId: string) => void;
  onRelay?: (event: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (
    ref: PresentationMessageRef,
    event: LedgerEvent,
    refIndex: number,
  ) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  showScrollButton: boolean;
  onScrollButtonClick: () => void;
  chatUnreadCount: number;
  onScrollChange?: (isAtBottom: boolean) => void;
  /** A context's target can be shown in live chat after its initial positioning. */
  onContextReady?: () => void;
  /** The current live view was opened on an explicit message. */
  liveJumpActive?: boolean;
  contextWindow?: boolean;
  /** The reader reached the end of a context window; initial means it opened there. */
  onReachedContextEnd?: (initial: boolean, atBottom: boolean) => boolean | void;
  onLoadNewerContext?: () => void;
  isLoadingNewerContext?: boolean;
  newerContextError?: boolean;
  onScrollSnapshot?: (snapshot: ChatScrollSnapshot, groupId?: string) => void;
  sendScrollRequest?: ChatSendScrollRequest | null;
  onSendScrollRequestConsumed?: (requestId: number) => void;
  isLoadingHistory?: boolean;
  hasMoreHistory?: boolean;
  /** The live list is narrowed by a message or sender filter. */
  hasActiveFilters?: boolean;
  isFilteredEmpty?: boolean;
  emptyMessage?: string;
  /** False means no request started; started requests continue to report state through props. */
  onLoadMore?: () => void | false | Promise<void | false>;
}
