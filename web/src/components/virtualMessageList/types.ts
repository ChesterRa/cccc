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
  highlightEventId?: string;
  className?: string;
  topInsetPx?: number;
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onReply: (event: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  onCopyContent?: (event: LedgerEvent) => void;
  onRelay?: (event: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  showScrollButton: boolean;
  onScrollButtonClick: () => void;
  chatUnreadCount: number;
  onScrollChange?: (isAtBottom: boolean) => void;
  onScrollSnapshot?: (snapshot: ChatScrollSnapshot, groupId?: string) => void;
  sendScrollRequest?: ChatSendScrollRequest | null;
  onSendScrollRequestConsumed?: (requestId: number) => void;
  isLoadingHistory?: boolean;
  hasMoreHistory?: boolean;
  isFilteredEmpty?: boolean;
  onLoadMore?: () => void;
}
