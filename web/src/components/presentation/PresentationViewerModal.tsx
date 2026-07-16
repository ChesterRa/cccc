import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUIStore } from "../../stores";
import { MarkdownDocumentSurface } from "../document/MarkdownDocumentSurface";
import {
  CloseIcon,
  CollapseIcon,
  CopyIcon,
  EditIcon,
  ExpandIcon,
  ImageIcon,
  MessageSquareTextIcon,
  RefreshIcon,
  SplitViewIcon,
  TrashIcon,
  WindowViewIcon,
} from "../Icons";
import { ModalFrame } from "../modals/ModalFrame";
import { useModalA11y } from "../../hooks/useModalA11y";
import type {
  GroupPresentation,
  LedgerEvent,
  PresentationMessageRef,
  PresentationSlot,
} from "../../types";
import {
  fetchPresentationBrowserSurfaceSession,
  getGroupBlobUrl,
  getPresentationAssetUrl,
  uploadPresentationReferenceSnapshot,
} from "../../services/api";
import { classNames } from "../../utils/classNames";
import { copyTextToClipboard } from "../../utils/copy";
import {
  findPresentationSlot,
  shouldPreferPresentationLiveBrowser,
} from "../../utils/presentation";
import {
  canRestorePresentationRefInViewer,
  getPresentationRefViewerScrollTop,
  shouldAutoOpenInteractivePresentation,
} from "../../utils/presentationLocator";
import { buildPresentationRefForSlot } from "../../utils/presentationRefs";
import {
  PresentationWebPreviewPanel,
  type PresentationWebPreviewMode,
} from "./PresentationWebPreviewPanel";
import type { PresentationBrowserFrame } from "./PresentationBrowserSurfacePanel";

type PresentationViewerBaseProps = {
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  slotId: string;
  presentation: GroupPresentation | null;
  sourceEvent?: LedgerEvent | null;
  focusRef?: PresentationMessageRef | null;
  focusEventId?: string | null;
  onQuoteInChat?: (payload: { slotId: string; ref?: PresentationMessageRef | null }) => void;
  onOpenMessageContext?: (eventId: string) => void;
  onReplyToMessage?: (event: LedgerEvent) => void;
  onReplaceSlot?: (slotId: string) => void;
  onClearSlot?: (slotId: string) => void;
  onClose: () => void;
};

type PresentationViewerProps = PresentationViewerBaseProps & {
  variant: "modal" | "split";
  isOpen?: boolean;
  supportsSplit?: boolean;
  onOpenSplit?: () => void;
  onOpenWindow?: () => void;
};

type PresentationViewerModalProps = PresentationViewerBaseProps & {
  isOpen: boolean;
  supportsSplit?: boolean;
  onOpenSplit?: () => void;
};

type PresentationViewerSplitPanelProps = PresentationViewerBaseProps & {
  onOpenWindow?: () => void;
};

function getReferenceHref(
  groupId: string,
  slot: PresentationSlot | null,
  cacheBust?: string | number,
): string {
  const card = slot?.card;
  if (!card) return "";
  const url = String(card.content.url || "").trim();
  if (url) return url;
  return getPresentationAssetUrl(groupId, slot.slot_id, cacheBust);
}

async function dataUrlToFile(dataUrl: string, filename: string): Promise<File | null> {
  const raw = String(dataUrl || "").trim();
  if (!raw) return null;
  try {
    const response = await fetch(raw);
    const blob = await response.blob();
    return new File([blob], filename, { type: blob.type || "image/jpeg" });
  } catch {
    return null;
  }
}

function formatTimestamp(value: string | undefined, locale: string): string {
  const raw = String(value || "").trim();
  if (!raw) return "";
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return "";
  return parsed.toLocaleString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function getCardTypeLabel(
  type: string,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (String(type || "").trim()) {
    case "markdown":
      return t("presentationTypeMarkdown", { defaultValue: "Markdown" });
    case "table":
      return t("presentationTypeTable", { defaultValue: "Table" });
    case "image":
      return t("presentationTypeImage", { defaultValue: "Image" });
    case "pdf":
      return t("presentationTypePdf", { defaultValue: "PDF" });
    case "web_preview":
      return t("presentationTypeWebPreview", { defaultValue: "Web" });
    default:
      return t("presentationTypeFile", { defaultValue: "File" });
  }
}

function PresentationWindowExpandIcon({ expanded }: { expanded: boolean }) {
  const Icon = expanded ? CollapseIcon : ExpandIcon;
  return <Icon aria-hidden="true" className="h-4 w-4" strokeWidth={1.6} />;
}

function PresentationViewer({
  variant,
  isOpen = true,
  supportsSplit = false,
  onOpenSplit,
  onOpenWindow,
  isDark,
  readOnly,
  groupId,
  slotId,
  presentation,
  sourceEvent,
  focusRef,
  focusEventId,
  onQuoteInChat,
  onOpenMessageContext,
  onReplyToMessage,
  onReplaceSlot,
  onClearSlot,
  onClose,
}: PresentationViewerProps) {
  const { t, i18n } = useTranslation("chat");
  const showError = useUIStore((state) => state.showError);
  const isModal = variant === "modal";
  const { modalRef } = useModalA11y(isModal && isOpen, onClose);
  const [refreshTick, setRefreshTick] = useState(0);
  const [isExpanded, setIsExpanded] = useState(false);
  const [linkedMarkdown, setLinkedMarkdown] = useState("");
  const [linkedMarkdownError, setLinkedMarkdownError] = useState("");
  const [copiedReference, setCopiedReference] = useState(false);
  const [quotePending, setQuotePending] = useState(false);
  const [browserFrameForQuote, setBrowserFrameForQuote] = useState<PresentationBrowserFrame | null>(
    null,
  );
  const [snapshotViewMode, setSnapshotViewMode] = useState<"hidden" | "compare" | "snapshot">(
    "hidden",
  );
  const [snapshotMobileTab, setSnapshotMobileTab] = useState<"live" | "snapshot">("live");
  const [snapshotLightboxOpen, setSnapshotLightboxOpen] = useState(false);
  const copyResetTimerRef = useRef<number | null>(null);
  const evidenceScrollRef = useRef<HTMLDivElement | null>(null);

  const slot = useMemo(() => findPresentationSlot(presentation, slotId), [presentation, slotId]);
  const card = slot?.card || null;
  const isWorkspaceLinked =
    !!card &&
    card.content.mode === "workspace_link" &&
    !!String(card.content.workspace_rel_path || "").trim();
  const cacheBust = isWorkspaceLinked
    ? `${card?.published_at || "linked"}:${refreshTick}`
    : undefined;
  const href = useMemo(
    () => getReferenceHref(groupId, slot, cacheBust),
    [cacheBust, groupId, slot],
  );
  const publishedAt = formatTimestamp(card?.published_at, i18n.language);
  const useSandboxedPreview =
    !!card && card.card_type === "web_preview" && !String(card.content.url || "").trim();
  const cardType = String(card?.card_type || "").trim();
  const cardMode = String(card?.content.mode || "inline").trim();
  const allowLiveBrowser =
    !!card &&
    card.card_type === "web_preview" &&
    !!String(card.content.url || "").trim() &&
    !readOnly;
  const preferredWebPreviewMode: PresentationWebPreviewMode =
    allowLiveBrowser && shouldPreferPresentationLiveBrowser(href) ? "interactive" : "embedded";
  const [webPreviewMode, setWebPreviewMode] =
    useState<PresentationWebPreviewMode>(preferredWebPreviewMode);
  const showWebPreviewModeToggle = !!card && card.card_type === "web_preview" && allowLiveBrowser;
  const canRefresh = !!card && (isWorkspaceLinked || card.card_type === "web_preview");
  const copyReferenceValue = String(
    card?.content.url || card?.content.workspace_rel_path || href || "",
  ).trim();
  const viewerPanelClassName = isExpanded
    ? "h-screen w-screen max-w-none sm:h-[96vh] sm:w-[96vw] sm:max-w-[96vw]"
    : "h-screen w-screen max-w-none sm:h-[88vh] sm:w-[min(1280px,96vw)]";
  const immersiveViewportClassName = "h-full min-h-0";
  const imageViewportClassName = isExpanded ? "max-h-[calc(96vh-14rem)]" : "max-h-[70vh]";
  const fullScreenLabel = isExpanded
    ? t("presentationExitFullScreenAction", { defaultValue: "Exit full screen" })
    : t("presentationFullScreenAction", { defaultValue: "Full screen" });
  const sourceEventId = String(sourceEvent?.id || focusEventId || "").trim();
  const canReuseFocusedReference = useMemo(() => {
    if (!focusRef) return false;
    if (String(focusRef.slot_id || "").trim() !== String(slotId || "").trim()) return false;
    if (!slot?.card) return true;
    const focusedCardType = String(focusRef.card_type || "").trim();
    const currentCardType = String(slot.card.card_type || "").trim();
    if (focusedCardType && currentCardType && focusedCardType !== currentCardType) {
      return false;
    }
    const focusedHref = String(focusRef.href || "").trim();
    if (focusedHref && href && focusedHref !== href) {
      return false;
    }
    return true;
  }, [focusRef, href, slot, slotId]);

  const currentRef = useMemo(() => {
    if (focusRef && canReuseFocusedReference) {
      return focusRef;
    }
    return buildPresentationRefForSlot(slot, { href });
  }, [canReuseFocusedReference, focusRef, href, slot]);
  const quotedSnapshot = focusRef?.snapshot || currentRef?.snapshot;
  const currentSnapshotUrl = useMemo(
    () => getGroupBlobUrl(groupId, String(quotedSnapshot?.path || "").trim()),
    [groupId, quotedSnapshot?.path],
  );
  const prefersInnerViewportScroll = cardType === "web_preview" || cardType === "pdf";
  const useOuterEvidenceScroll = !prefersInnerViewportScroll;
  const canRestoreRefInViewer = useMemo(
    () => canRestorePresentationRefInViewer(cardType),
    [cardType],
  );
  const targetViewerScrollTop = useMemo(
    () => getPresentationRefViewerScrollTop(currentRef),
    [currentRef],
  );
  const quoteStillMatchesLive = !!card && (!focusRef || canReuseFocusedReference);
  const canCompareSnapshot = !!currentSnapshotUrl && quoteStillMatchesLive;
  const quoteContextChanged = !!focusRef && !quoteStillMatchesLive;
  const showSnapshotCompare = snapshotViewMode === "compare" && canCompareSnapshot;
  const showSnapshotOverlay = snapshotViewMode === "snapshot" && !!currentSnapshotUrl;
  const snapshotTimestamp = formatTimestamp(quotedSnapshot?.captured_at, i18n.language);
  const snapshotToggleLabel =
    snapshotViewMode === "hidden"
      ? canCompareSnapshot
        ? t("presentationCompareSnapshotAction", { defaultValue: "Compare with snapshot" })
        : t("presentationOpenQuotedSnapshotAction", { defaultValue: "Open quoted snapshot" })
      : t("presentationHideSnapshotAction", { defaultValue: "Hide snapshot" });
  const iconButtonClassName = classNames(
    "inline-flex h-9 w-9 items-center justify-center rounded-full border transition-colors",
    isDark
      ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
      : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
  );
  const destructiveIconButtonClassName = classNames(
    "inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors",
    isDark
      ? "bg-rose-500/15 text-rose-200 hover:bg-rose-500/25"
      : "bg-rose-50 text-rose-700 hover:bg-rose-100",
  );
  const copiedIconButtonClassName = classNames(
    "inline-flex h-9 w-9 items-center justify-center rounded-full border transition-colors",
    isDark
      ? "border-white/16 bg-white/[0.12] text-white"
      : "border-black/10 bg-white text-[rgb(35,36,37)]",
  );
  const refreshActionLabel = t("presentationRefreshAction", { defaultValue: "Refresh" });
  const copyActionLabel = copiedReference
    ? t("presentationCopyReferenceCopied", { defaultValue: "Copied" })
    : t("presentationCopyReferenceAction", { defaultValue: "Copy URL/path" });
  const editActionLabel = t("presentationReplaceAction", { defaultValue: "Edit" });
  const clearActionLabel = t("presentationClearAction", { defaultValue: "Clear" });
  const embeddedModeLabel = t("presentationEmbeddedModeLabel", { defaultValue: "Standard" });
  const interactiveModeLabel = t("presentationInteractiveModeLabel", { defaultValue: "Enhanced" });
  const previewModeLabel = t("presentationPreviewModeLabel", { defaultValue: "Web preview mode" });
  const embeddedModeHelp = t("presentationEmbeddedModeHelp", {
    defaultValue:
      "Standard mode is lightweight. If links jump out or the page cannot load, switch to enhanced mode.",
  });
  const interactiveModeHelp = t("presentationInteractiveModeHelp", {
    defaultValue:
      "Enhanced mode works better for local or private pages and tries to keep navigation inside CCCC.",
  });

  const modalHeaderActions = (
    <>
      {sourceEventId && onOpenMessageContext ? (
        <button
          type="button"
          onClick={() => onOpenMessageContext(sourceEventId)}
          className={classNames(
            "inline-flex min-h-[40px] items-center justify-center rounded-lg border px-3 text-sm font-medium transition-colors",
            isDark
              ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
          )}
        >
          {t("presentationJumpToChatAction", { defaultValue: "Jump to chat" })}
        </button>
      ) : null}
      {sourceEvent && onReplyToMessage ? (
        <button
          type="button"
          onClick={() => onReplyToMessage(sourceEvent)}
          className={classNames(
            "inline-flex min-h-[40px] items-center justify-center rounded-lg border px-3 text-sm font-medium transition-colors",
            isDark
              ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
          )}
        >
          {t("presentationReplyInChatAction", { defaultValue: "Reply in chat" })}
        </button>
      ) : null}
      {canRefresh ? (
        <button
          type="button"
          onClick={() => setRefreshTick((value) => value + 1)}
          className={classNames(
            "inline-flex min-h-[40px] min-w-[40px] items-center justify-center rounded-lg border transition-colors",
            isDark
              ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
          )}
          aria-label={refreshActionLabel}
          title={refreshActionLabel}
        >
          <RefreshIcon size={16} />
        </button>
      ) : null}
      {card ? (
        <button
          type="button"
          onClick={() => setIsExpanded((value) => !value)}
          className={classNames(
            "hidden sm:inline-flex min-h-[40px] min-w-[40px] items-center justify-center rounded-lg border transition-colors",
            isDark
              ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
          )}
          aria-label={fullScreenLabel}
          title={fullScreenLabel}
        >
          <PresentationWindowExpandIcon expanded={isExpanded} />
        </button>
      ) : null}
      {supportsSplit && onOpenSplit ? (
        <button
          type="button"
          onClick={onOpenSplit}
          className={classNames(
            "inline-flex min-h-[40px] min-w-[40px] items-center justify-center rounded-lg border transition-colors",
            isDark
              ? "border-white/12 bg-white/[0.06] text-white hover:bg-white/[0.1]"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white",
          )}
          aria-label={t("presentationOpenSplitViewAction", { defaultValue: "Open beside chat" })}
          title={t("presentationOpenSplitViewAction", { defaultValue: "Open beside chat" })}
        >
          <SplitViewIcon size={16} />
        </button>
      ) : null}
    </>
  );

  useEffect(() => {
    setWebPreviewMode(preferredWebPreviewMode);
  }, [preferredWebPreviewMode, slotId, card?.published_at]);

  useEffect(() => {
    let cancelled = false;
    if (!allowLiveBrowser) return undefined;

    const run = async () => {
      const existing = await fetchPresentationBrowserSurfaceSession(groupId, slotId);
      if (cancelled || !existing.ok) return;
      if (
        shouldAutoOpenInteractivePresentation(allowLiveBrowser, existing.result.browser_surface)
      ) {
        setWebPreviewMode("interactive");
      }
    };

    void run();
    return () => {
      cancelled = true;
    };
  }, [allowLiveBrowser, groupId, slotId, card?.published_at]);

  useEffect(() => {
    if (!isOpen || !isWorkspaceLinked) return;
    const timer = window.setInterval(() => {
      setRefreshTick((value) => value + 1);
    }, 5000);
    return () => window.clearInterval(timer);
  }, [isOpen, isWorkspaceLinked, slotId, card?.published_at]);

  useEffect(() => {
    if (!isOpen || cardType !== "markdown") return;
    if (cardMode === "inline") return;
    if (!href) {
      setLinkedMarkdown("");
      setLinkedMarkdownError("");
      return;
    }

    const controller = new AbortController();
    let active = true;

    const run = async () => {
      try {
        const resp = await fetch(href, { cache: "no-store", signal: controller.signal });
        if (!resp.ok) {
          throw new Error(`HTTP ${resp.status}`);
        }
        const text = await resp.text();
        if (!active) return;
        setLinkedMarkdown(text);
        setLinkedMarkdownError("");
      } catch (error) {
        if (!active || controller.signal.aborted) return;
        setLinkedMarkdown("");
        setLinkedMarkdownError(error instanceof Error ? error.message : String(error));
      }
    };

    void run();
    return () => {
      active = false;
      controller.abort();
    };
  }, [cardMode, cardType, href, isOpen]);

  useEffect(() => {
    return () => {
      if (copyResetTimerRef.current !== null) {
        window.clearTimeout(copyResetTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (variant !== "modal") {
      setIsExpanded(false);
    }
  }, [variant]);

  useEffect(() => {
    if (!isOpen) {
      setSnapshotViewMode("hidden");
      setSnapshotMobileTab("live");
      setSnapshotLightboxOpen(false);
      return;
    }
    if (!currentSnapshotUrl) {
      setSnapshotViewMode("hidden");
      setSnapshotMobileTab("live");
      setSnapshotLightboxOpen(false);
      return;
    }
    setSnapshotViewMode("hidden");
    setSnapshotMobileTab("live");
    setSnapshotLightboxOpen(false);
  }, [currentSnapshotUrl, focusEventId, isOpen, slotId]);

  useEffect(() => {
    if (snapshotViewMode === "compare" && !canCompareSnapshot) {
      setSnapshotViewMode(currentSnapshotUrl ? "snapshot" : "hidden");
    }
  }, [canCompareSnapshot, currentSnapshotUrl, snapshotViewMode]);

  useEffect(() => {
    if (!isOpen || !canRestoreRefInViewer || targetViewerScrollTop == null) return;

    let timeoutId: number | null = null;
    let rafIdOne: number | null = null;
    let rafIdTwo: number | null = null;

    const applyScrollRestore = () => {
      const el = evidenceScrollRef.current;
      if (!el) return;
      el.scrollTop = targetViewerScrollTop;
    };

    timeoutId = window.setTimeout(() => {
      applyScrollRestore();
      rafIdOne = window.requestAnimationFrame(() => {
        applyScrollRestore();
        rafIdTwo = window.requestAnimationFrame(() => {
          applyScrollRestore();
        });
      });
    }, 0);

    return () => {
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
      if (rafIdOne !== null) {
        window.cancelAnimationFrame(rafIdOne);
      }
      if (rafIdTwo !== null) {
        window.cancelAnimationFrame(rafIdTwo);
      }
    };
  }, [
    canRestoreRefInViewer,
    isOpen,
    linkedMarkdown,
    linkedMarkdownError,
    slotId,
    targetViewerScrollTop,
  ]);

  const handleCopyReference = async () => {
    if (!copyReferenceValue) return;
    const ok = await copyTextToClipboard(copyReferenceValue);
    if (!ok) {
      showError(t("common:copyFailed", { defaultValue: "Copy failed" }));
      return;
    }
    setCopiedReference(true);
    if (copyResetTimerRef.current !== null) {
      window.clearTimeout(copyResetTimerRef.current);
    }
    copyResetTimerRef.current = window.setTimeout(() => {
      copyResetTimerRef.current = null;
      setCopiedReference(false);
    }, 1600);
  };

  const handleQuoteInChat = async () => {
    if (!slot?.card || !onQuoteInChat || quotePending) return;
    const activeCard = slot.card;
    const viewerScrollTop = evidenceScrollRef.current?.scrollTop || 0;
    const nextLocator: Record<string, unknown> = {};
    if (viewerScrollTop > 0) {
      nextLocator.viewer_scroll_top = viewerScrollTop;
    }
    if (activeCard.card_type === "web_preview") {
      const browserUrl = String(browserFrameForQuote?.url || href || "").trim();
      if (browserUrl) nextLocator.url = browserUrl;
      const capturedAt = String(browserFrameForQuote?.capturedAt || "").trim();
      if (capturedAt) nextLocator.captured_at = capturedAt;
    }

    let snapshot = undefined;
    if (activeCard.card_type === "web_preview" && browserFrameForQuote?.dataUrl) {
      setQuotePending(true);
      try {
        const extension = browserFrameForQuote.dataUrl.includes("image/png") ? "png" : "jpg";
        const file = await dataUrlToFile(
          browserFrameForQuote.dataUrl,
          `presentation-ref-${String(slot.slot_id || "slot").trim()}.${extension}`,
        );
        if (file) {
          const upload = await uploadPresentationReferenceSnapshot(groupId, {
            slotId: slot.slot_id,
            file,
            source: "browser_surface",
            capturedAt: browserFrameForQuote.capturedAt,
            width: browserFrameForQuote.width,
            height: browserFrameForQuote.height,
          });
          if (upload.ok) {
            snapshot = upload.result.snapshot;
          }
        }
      } finally {
        setQuotePending(false);
      }
    }

    const ref = buildPresentationRefForSlot(slot, {
      href,
      status: "open",
      locator: Object.keys(nextLocator).length > 0 ? nextLocator : undefined,
      snapshot,
    });
    if (!ref) return;
    onQuoteInChat({ slotId: slot.slot_id, ref });
  };

  const handleToggleSnapshotView = () => {
    if (!currentSnapshotUrl) return;
    if (snapshotViewMode !== "hidden") {
      setSnapshotViewMode("hidden");
      setSnapshotMobileTab("live");
      return;
    }
    if (canCompareSnapshot) {
      setIsExpanded(true);
    }
    setSnapshotViewMode(canCompareSnapshot ? "compare" : "snapshot");
    setSnapshotMobileTab("live");
  };

  const snapshotPanel = currentSnapshotUrl ? (
    <div
      className={classNames(
        "flex h-full min-h-0 flex-col overflow-hidden rounded-3xl border",
        isDark ? "border-white/10 bg-slate-950/60" : "border-black/10 bg-white/92",
      )}
    >
      <div
        className={classNames(
          "flex items-start justify-between gap-3 border-b px-4 py-3",
          isDark ? "border-white/10" : "border-black/10",
        )}
      >
        <div className="min-w-0">
          <div
            className={classNames(
              "text-xs font-semibold uppercase tracking-[0.16em]",
              isDark ? "text-white/85" : "text-[rgb(35,36,37)]/85",
            )}
          >
            {t("presentationSnapshotFromQuoteLabel", { defaultValue: "Snapshot from this quote" })}
          </div>
          {snapshotTimestamp ? (
            <div
              className={classNames("mt-1 text-xs", isDark ? "text-slate-400" : "text-gray-500")}
            >
              {snapshotTimestamp}
            </div>
          ) : null}
          {quoteContextChanged ? (
            <div
              className={classNames(
                "mt-1 text-xs",
                isDark ? "text-amber-300/90" : "text-amber-700",
              )}
            >
              {t("presentationSnapshotLiveChangedHint", {
                defaultValue: "The current slot no longer matches this quote.",
              })}
            </div>
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => setSnapshotLightboxOpen(true)}
          className={classNames(
            "inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors",
            isDark
              ? "bg-white/5 text-slate-300 hover:bg-white/10 hover:text-slate-100"
              : "bg-black/5 text-gray-600 hover:bg-black/10 hover:text-gray-900",
          )}
          aria-label={t("presentationOpenSnapshotLightboxAction", {
            defaultValue: "Open snapshot",
          })}
          title={t("presentationOpenSnapshotLightboxAction", { defaultValue: "Open snapshot" })}
        >
          <ExpandIcon aria-hidden="true" className="h-4 w-4" strokeWidth={1.6} />
        </button>
      </div>
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-3">
        <button
          type="button"
          onClick={() => setSnapshotLightboxOpen(true)}
          className="flex h-full min-h-[240px] w-full items-center justify-center"
          aria-label={t("presentationOpenSnapshotLightboxAction", {
            defaultValue: "Open snapshot",
          })}
        >
          <img
            src={currentSnapshotUrl}
            alt={t("presentationQuotedSnapshotAlt", { defaultValue: "Quoted snapshot" })}
            className="max-h-full w-full rounded-2xl border border-[var(--glass-border-subtle)] object-contain bg-black/5"
          />
        </button>
      </div>
    </div>
  ) : null;

  const evidencePanel = !card ? (
    <div
      className={classNames(
        "flex h-full min-h-[320px] items-center justify-center rounded-3xl border border-dashed text-sm",
        isDark ? "border-white/10 text-slate-500" : "border-black/10 text-gray-500",
      )}
    >
      {t("presentationMissingCard", { defaultValue: "This presentation slot is empty." })}
    </div>
  ) : card.card_type === "markdown" ? (
    <MarkdownDocumentSurface
      content={String(
        card.content.mode === "inline" ? card.content.markdown || "" : linkedMarkdown || "",
      )}
      error={linkedMarkdownError}
      isDark={isDark}
    />
  ) : card.card_type === "table" ? (
    <div
      className={classNames(
        "overflow-hidden rounded-3xl border",
        isDark ? "border-white/10 bg-slate-950/60" : "border-black/10 bg-white/95",
      )}
    >
      <div className="overflow-auto">
        <table className="min-w-full border-collapse text-sm">
          <thead className={isDark ? "bg-slate-900/80 text-slate-200" : "bg-gray-50 text-gray-800"}>
            <tr>
              {(card.content.table?.columns || []).map((column) => (
                <th
                  key={column}
                  className="border-b border-inherit px-4 py-3 text-left font-semibold"
                >
                  {column}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className={isDark ? "text-slate-300" : "text-gray-700"}>
            {(card.content.table?.rows || []).map((row, rowIndex) => (
              <tr
                key={`row-${rowIndex}`}
                className={
                  rowIndex % 2 === 0
                    ? isDark
                      ? "bg-slate-950/40"
                      : "bg-white"
                    : isDark
                      ? "bg-slate-900/30"
                      : "bg-gray-50/80"
                }
              >
                {row.map((cell, cellIndex) => (
                  <td
                    key={`cell-${rowIndex}:${cellIndex}`}
                    className="border-b border-[var(--glass-border-subtle)] px-4 py-3 align-top"
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  ) : card.card_type === "image" ? (
    <div className="flex min-h-[360px] items-center justify-center">
      <img
        src={href}
        alt={card.title}
        className={classNames(
          imageViewportClassName,
          "max-w-full rounded-3xl border border-[var(--glass-border-subtle)] object-contain shadow-xl",
        )}
      />
    </div>
  ) : card.card_type === "pdf" ? (
    <iframe
      title={card.title}
      src={href}
      className={classNames(
        immersiveViewportClassName,
        "w-full rounded-3xl border border-[var(--glass-border-subtle)] bg-white",
      )}
    />
  ) : card.card_type === "web_preview" ? (
    isOpen ? (
      <PresentationWebPreviewPanel
        key={`${slot?.slot_id || ""}:${card.published_at}:${href}`}
        groupId={groupId}
        slotId={slot?.slot_id || ""}
        title={card.title}
        href={href}
        isDark={isDark}
        useSandboxedPreview={useSandboxedPreview}
        allowLiveBrowser={allowLiveBrowser}
        mode={webPreviewMode}
        refreshNonce={refreshTick}
        viewportClassName={immersiveViewportClassName}
        onInteractiveFrameUpdate={setBrowserFrameForQuote}
      />
    ) : null
  ) : (
    <div
      className={classNames(
        "rounded-3xl border p-6",
        isDark ? "border-white/10 bg-slate-950/60" : "border-black/10 bg-white/90",
      )}
    >
      <div
        className={classNames(
          "text-base font-semibold",
          isDark ? "text-slate-100" : "text-gray-900",
        )}
      >
        {card.title}
      </div>
      <div className={classNames("mt-2 text-sm", isDark ? "text-slate-400" : "text-gray-600")}>
        {card.summary ||
          card.source_label ||
          t("presentationFileReady", {
            defaultValue: "This file cannot be previewed in Presentation yet.",
          })}
      </div>
    </div>
  );

  const viewerBody = (
    <div className="flex min-h-0 flex-1 flex-col">
      {variant === "modal" ? (
        <div
          className={classNames(
            "flex flex-wrap items-center gap-2 border-b px-3 py-2 text-xs",
            isDark ? "border-white/10 text-slate-400" : "border-black/10 text-gray-600",
          )}
        >
          {card ? (
            <>
              <span
                className={classNames(
                  "rounded-full px-2 py-1 font-medium",
                  isDark
                    ? "bg-white/[0.08] text-white"
                    : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                )}
              >
                {getCardTypeLabel(card.card_type, t)}
              </span>
              {isWorkspaceLinked ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-1 font-medium",
                    isDark
                      ? "bg-emerald-500/10 text-emerald-200"
                      : "bg-emerald-50 text-emerald-700",
                  )}
                >
                  {t("presentationWorkspaceLiveBadge", { defaultValue: "Live workspace link" })}
                </span>
              ) : null}
              {card.source_label ? <span>{card.source_label}</span> : null}
              {publishedAt ? <span>{publishedAt}</span> : null}
              <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
                {showWebPreviewModeToggle ? (
                  <div
                    className={classNames(
                      "inline-flex items-center rounded-full border p-0.5",
                      isDark
                        ? "border-white/10 bg-white/[0.04]"
                        : "border-black/10 bg-black/[0.03]",
                    )}
                    role="group"
                    aria-label={previewModeLabel}
                  >
                    <button
                      type="button"
                      onClick={() => setWebPreviewMode("embedded")}
                      className={classNames(
                        "rounded-full px-2.5 py-1 text-[11px] font-medium whitespace-nowrap transition-colors",
                        webPreviewMode === "embedded"
                          ? isDark
                            ? "bg-slate-100 text-slate-950"
                            : "bg-slate-900 text-white"
                          : isDark
                            ? "text-slate-300 hover:bg-white/8"
                            : "text-gray-600 hover:bg-black/6",
                      )}
                      aria-pressed={webPreviewMode === "embedded"}
                      title={embeddedModeHelp}
                    >
                      {embeddedModeLabel}
                    </button>
                    <button
                      type="button"
                      onClick={() => setWebPreviewMode("interactive")}
                      className={classNames(
                        "rounded-full px-2.5 py-1 text-[11px] font-medium whitespace-nowrap transition-colors",
                        webPreviewMode === "interactive"
                          ? isDark
                            ? "bg-white/[0.08] text-white"
                            : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
                          : isDark
                            ? "text-slate-300 hover:bg-white/8"
                            : "text-gray-600 hover:bg-black/6",
                      )}
                      aria-pressed={webPreviewMode === "interactive"}
                      title={interactiveModeHelp}
                    >
                      {interactiveModeLabel}
                    </button>
                  </div>
                ) : null}
                {copyReferenceValue ? (
                  <button
                    type="button"
                    onClick={() => {
                      void handleCopyReference();
                    }}
                    className={copiedReference ? copiedIconButtonClassName : iconButtonClassName}
                    aria-label={copyActionLabel}
                    title={copyActionLabel}
                  >
                    <CopyIcon size={16} />
                  </button>
                ) : null}
                {!readOnly && onReplaceSlot ? (
                  <button
                    type="button"
                    onClick={() => slot && onReplaceSlot(slot.slot_id)}
                    className={iconButtonClassName}
                    aria-label={editActionLabel}
                    title={editActionLabel}
                  >
                    <EditIcon size={16} />
                  </button>
                ) : null}
                {!readOnly && onClearSlot ? (
                  <button
                    type="button"
                    onClick={() => slot && onClearSlot(slot.slot_id)}
                    className={destructiveIconButtonClassName}
                    aria-label={clearActionLabel}
                    title={clearActionLabel}
                  >
                    <TrashIcon size={16} />
                  </button>
                ) : null}
              </div>
            </>
          ) : (
            <span>
              {t("presentationMissingCard", { defaultValue: "This presentation slot is empty." })}
            </span>
          )}
        </div>
      ) : null}

      <div
        className={classNames(
          "relative min-h-0 flex-1 overflow-hidden",
          variant === "split" ? "px-2 py-2" : "px-4 py-3",
        )}
      >
        {showSnapshotCompare ? (
          <div className="mb-3 flex items-center gap-2 lg:hidden">
            <button
              type="button"
              onClick={() => setSnapshotMobileTab("live")}
              className={classNames(
                "rounded-full px-3 py-1.5 text-xs font-medium transition-colors",
                snapshotMobileTab === "live"
                  ? isDark
                    ? "bg-slate-100 text-slate-950"
                    : "bg-slate-900 text-white"
                  : isDark
                    ? "bg-white/5 text-slate-300 hover:bg-white/10"
                    : "bg-black/5 text-gray-600 hover:bg-black/10",
              )}
            >
              {t("presentationCurrentLiveViewLabel", { defaultValue: "Current view" })}
            </button>
            <button
              type="button"
              onClick={() => setSnapshotMobileTab("snapshot")}
              className={classNames(
                "rounded-full px-3 py-1.5 text-xs font-medium transition-colors",
                snapshotMobileTab === "snapshot"
                  ? isDark
                    ? "bg-slate-100 text-slate-950"
                    : "bg-slate-900 text-white"
                  : isDark
                    ? "bg-white/5 text-slate-300 hover:bg-white/10"
                    : "bg-black/5 text-gray-600 hover:bg-black/10",
              )}
            >
              {t("presentationSnapshotTabLabel", { defaultValue: "Snapshot" })}
            </button>
          </div>
        ) : null}
        <div
          className={classNames(
            "h-full min-h-0",
            showSnapshotCompare
              ? "grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,0.72fr)]"
              : "",
          )}
        >
          <div
            ref={evidenceScrollRef}
            className={classNames(
              "h-full min-h-0",
              useOuterEvidenceScroll ? "overflow-auto" : "overflow-hidden",
              !useOuterEvidenceScroll && card ? "flex flex-col" : "",
              useOuterEvidenceScroll && !readOnly && card && onQuoteInChat ? "pb-20" : "",
              showSnapshotCompare && snapshotMobileTab !== "live" ? "hidden lg:block" : "",
            )}
          >
            {evidencePanel}
          </div>
          {showSnapshotCompare ? (
            <div
              className={classNames(
                "min-h-0",
                snapshotMobileTab !== "snapshot" ? "hidden lg:block" : "",
              )}
            >
              {snapshotPanel}
            </div>
          ) : null}
        </div>
        {showSnapshotOverlay && snapshotPanel ? (
          <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/20 p-4 backdrop-blur-[2px]">
            <button
              type="button"
              className="absolute inset-0"
              onClick={() => setSnapshotViewMode("hidden")}
              aria-label={t("presentationCloseSnapshotAction", { defaultValue: "Close snapshot" })}
            />
            <div className="relative z-10 h-full min-h-0 w-full max-w-5xl">{snapshotPanel}</div>
          </div>
        ) : null}
        {snapshotLightboxOpen && currentSnapshotUrl ? (
          <div className="absolute inset-0 z-30 flex items-center justify-center p-4 sm:p-6">
            <button
              type="button"
              className="absolute inset-0 bg-black/60 backdrop-blur-sm"
              onClick={() => setSnapshotLightboxOpen(false)}
              aria-label={t("presentationCloseSnapshotAction", { defaultValue: "Close snapshot" })}
            />
            <div
              className={classNames(
                "relative z-10 flex h-full max-h-full w-full max-w-6xl min-h-0 flex-col overflow-hidden rounded-3xl border shadow-2xl",
                isDark ? "border-white/10 bg-slate-950/96" : "border-black/10 bg-white/96",
              )}
            >
              <div
                className={classNames(
                  "flex items-start justify-between gap-3 border-b px-4 py-3",
                  isDark ? "border-white/10" : "border-black/10",
                )}
              >
                <div className="min-w-0">
                  <div
                    className={classNames(
                      "text-sm font-semibold",
                      isDark ? "text-slate-100" : "text-gray-900",
                    )}
                  >
                    {t("presentationSnapshotFromQuoteLabel", {
                      defaultValue: "Snapshot from this quote",
                    })}
                  </div>
                  {snapshotTimestamp ? (
                    <div
                      className={classNames(
                        "mt-1 text-xs",
                        isDark ? "text-slate-400" : "text-gray-500",
                      )}
                    >
                      {snapshotTimestamp}
                    </div>
                  ) : null}
                  {quoteContextChanged ? (
                    <div
                      className={classNames(
                        "mt-1 text-xs",
                        isDark ? "text-amber-300/90" : "text-amber-700",
                      )}
                    >
                      {t("presentationSnapshotLiveChangedHint", {
                        defaultValue: "The current slot no longer matches this quote.",
                      })}
                    </div>
                  ) : null}
                </div>
                <button
                  type="button"
                  onClick={() => setSnapshotLightboxOpen(false)}
                  className={classNames(
                    "inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors",
                    isDark
                      ? "bg-white/5 text-slate-300 hover:bg-white/10 hover:text-slate-100"
                      : "bg-black/5 text-gray-600 hover:bg-black/10 hover:text-gray-900",
                  )}
                  aria-label={t("presentationCloseSnapshotAction", {
                    defaultValue: "Close snapshot",
                  })}
                  title={t("presentationCloseSnapshotAction", { defaultValue: "Close snapshot" })}
                >
                  <CloseIcon aria-hidden="true" className="h-4 w-4" strokeWidth={1.6} />
                </button>
              </div>
              <div className="min-h-0 flex-1 overflow-auto bg-black/10 p-4">
                <img
                  src={currentSnapshotUrl}
                  alt={t("presentationQuotedSnapshotAlt", { defaultValue: "Quoted snapshot" })}
                  className="mx-auto h-auto max-h-full max-w-full rounded-2xl object-contain"
                />
              </div>
            </div>
          </div>
        ) : null}
        {!readOnly && card && onQuoteInChat ? (
          <div className="pointer-events-none absolute bottom-[max(1rem,env(safe-area-inset-bottom))] right-5 z-10 flex items-center gap-2">
            {currentSnapshotUrl ? (
              <button
                type="button"
                onClick={handleToggleSnapshotView}
                className={classNames(
                  "pointer-events-auto inline-flex h-10 items-center gap-2 rounded-full border px-3 text-sm font-medium shadow-lg backdrop-blur-xl transition-colors",
                  snapshotViewMode !== "hidden"
                    ? isDark
                      ? "border-white/12 bg-white/[0.08] text-white hover:bg-white/[0.12]"
                      : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-[rgb(240,240,240)]"
                    : isDark
                      ? "border-white/10 bg-slate-900/78 text-slate-200 hover:bg-slate-900"
                      : "border-black/10 bg-white/88 text-gray-700 hover:bg-white",
                )}
                aria-label={snapshotToggleLabel}
                title={snapshotToggleLabel}
              >
                <ImageIcon aria-hidden="true" className="h-4 w-4" strokeWidth={1.5} />
                <span>{snapshotToggleLabel}</span>
              </button>
            ) : null}
            <button
              type="button"
              onClick={() => {
                void handleQuoteInChat();
              }}
              disabled={quotePending}
              className={classNames(
                "pointer-events-auto inline-flex h-10 items-center gap-2 rounded-full border px-3.5 text-sm font-medium shadow-lg backdrop-blur-xl transition-colors",
                isDark
                  ? "border-white/10 bg-slate-900/82 text-white hover:bg-slate-900"
                  : "border-black/10 bg-white/88 text-[rgb(35,36,37)] hover:bg-white",
                quotePending ? "opacity-70" : "",
              )}
              aria-label={t("presentationQuoteInChatAction", { defaultValue: "Quote in chat" })}
              title={t("presentationQuoteInChatAction", { defaultValue: "Quote in chat" })}
            >
              <MessageSquareTextIcon aria-hidden="true" className="h-4 w-4" strokeWidth={1.5} />
              <span>
                {quotePending
                  ? t("presentationQuotePendingAction", { defaultValue: "Quoting..." })
                  : t("presentationQuoteAction", { defaultValue: "Quote" })}
              </span>
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );

  if (variant === "split") {
    return (
      <section
        className={classNames(
          "flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
          isDark ? "bg-slate-950/18" : "bg-white/62",
        )}
        aria-label={t("presentationTitle", { defaultValue: "Presentation" })}
      >
        <div
          className={classNames(
            "flex items-center justify-between gap-2 border-b px-3 py-1.5",
            isDark ? "border-white/8" : "border-black/8",
          )}
        >
          <div className="min-w-0 flex items-center gap-2">
            <div
              className={classNames(
                "truncate text-sm font-semibold",
                isDark ? "text-slate-100" : "text-gray-900",
              )}
            >
              {card?.title || t("presentationTitle", { defaultValue: "Presentation" })}
            </div>
            {card ? (
              <span
                className={classNames(
                  "flex-shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium",
                  isDark
                    ? "bg-white/[0.08] text-white"
                    : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                )}
              >
                {getCardTypeLabel(card.card_type, t)}
              </span>
            ) : null}
          </div>
          <div className="flex flex-shrink-0 items-center gap-1">
            {showWebPreviewModeToggle ? (
              <div
                className={classNames(
                  "inline-flex items-center rounded-full border p-0.5",
                  isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-black/[0.03]",
                )}
                role="group"
                aria-label={previewModeLabel}
              >
                <button
                  type="button"
                  onClick={() => setWebPreviewMode("embedded")}
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-[10px] font-medium whitespace-nowrap transition-colors",
                    webPreviewMode === "embedded"
                      ? isDark
                        ? "bg-slate-100 text-slate-950"
                        : "bg-slate-900 text-white"
                      : isDark
                        ? "text-slate-300 hover:bg-white/8"
                        : "text-gray-600 hover:bg-black/6",
                  )}
                  aria-pressed={webPreviewMode === "embedded"}
                  title={embeddedModeHelp}
                >
                  {embeddedModeLabel}
                </button>
                <button
                  type="button"
                  onClick={() => setWebPreviewMode("interactive")}
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-[10px] font-medium whitespace-nowrap transition-colors",
                    webPreviewMode === "interactive"
                      ? isDark
                        ? "bg-white/[0.08] text-white"
                        : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
                      : isDark
                        ? "text-slate-300 hover:bg-white/8"
                        : "text-gray-600 hover:bg-black/6",
                  )}
                  aria-pressed={webPreviewMode === "interactive"}
                  title={interactiveModeHelp}
                >
                  {interactiveModeLabel}
                </button>
              </div>
            ) : null}
            {canRefresh ? (
              <button
                type="button"
                onClick={() => setRefreshTick((value) => value + 1)}
                className={classNames(
                  "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                  isDark
                    ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                    : "bg-gray-100 text-gray-800 hover:bg-gray-200",
                )}
                aria-label={refreshActionLabel}
                title={refreshActionLabel}
              >
                <RefreshIcon size={14} />
              </button>
            ) : null}
            {copyReferenceValue ? (
              <button
                type="button"
                onClick={() => {
                  void handleCopyReference();
                }}
                className={classNames(
                  "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                  copiedReference
                    ? isDark
                      ? "bg-white/[0.08] text-white"
                      : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
                    : isDark
                      ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                      : "bg-gray-100 text-gray-800 hover:bg-gray-200",
                )}
                aria-label={copyActionLabel}
                title={copyActionLabel}
              >
                <CopyIcon size={14} />
              </button>
            ) : null}
            {onOpenWindow ? (
              <button
                type="button"
                onClick={onOpenWindow}
                className={classNames(
                  "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                  isDark
                    ? "bg-slate-800 text-white hover:bg-slate-700 hover:text-white"
                    : "bg-gray-100 text-[rgb(35,36,37)] hover:bg-gray-200 hover:text-black",
                )}
                aria-label={t("presentationOpenWindowAction", { defaultValue: "Open in window" })}
                title={t("presentationOpenWindowAction", { defaultValue: "Open in window" })}
              >
                <WindowViewIcon size={14} />
              </button>
            ) : null}
            {!readOnly && onReplaceSlot && slot ? (
              <button
                type="button"
                onClick={() => onReplaceSlot(slot.slot_id)}
                className={classNames(
                  "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                  isDark
                    ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                    : "bg-gray-100 text-gray-800 hover:bg-gray-200",
                )}
                aria-label={editActionLabel}
                title={editActionLabel}
              >
                <EditIcon size={14} />
              </button>
            ) : null}
            {!readOnly && onClearSlot && slot ? (
              <button
                type="button"
                onClick={() => onClearSlot(slot.slot_id)}
                className={classNames(
                  "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                  isDark
                    ? "bg-rose-500/15 text-rose-200 hover:bg-rose-500/25"
                    : "bg-rose-50 text-rose-700 hover:bg-rose-100",
                )}
                aria-label={clearActionLabel}
                title={clearActionLabel}
              >
                <TrashIcon size={14} />
              </button>
            ) : null}
            <button
              type="button"
              onClick={onClose}
              className={classNames(
                "inline-flex h-8 w-8 items-center justify-center rounded-full transition-colors",
                isDark
                  ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                  : "bg-gray-100 text-gray-800 hover:bg-gray-200",
              )}
              aria-label={t("presentationCloseSplitAction", { defaultValue: "Close presentation" })}
              title={t("presentationCloseSplitAction", { defaultValue: "Close presentation" })}
            >
              <CloseIcon size={14} />
            </button>
          </div>
        </div>
        {viewerBody}
      </section>
    );
  }

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="presentation-viewer-title"
      title={card?.title || t("presentationTitle", { defaultValue: "Presentation" })}
      closeAriaLabel={t("presentationCloseViewer", { defaultValue: "Close presentation viewer" })}
      panelClassName={viewerPanelClassName}
      headerActions={modalHeaderActions}
      modalRef={modalRef}
    >
      {viewerBody}
    </ModalFrame>
  );
}

export function PresentationViewerModal(props: PresentationViewerModalProps) {
  return <PresentationViewer variant="modal" {...props} />;
}

export function PresentationViewerSplitPanel(props: PresentationViewerSplitPanelProps) {
  return <PresentationViewer variant="split" {...props} />;
}
