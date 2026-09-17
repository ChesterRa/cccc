import { buttonVariants } from "../ui/button-variants";
import { PanelRightClose } from "lucide-react";
import { PresentationSlotNavigation } from "./PresentationSlotNavigation";
import { GraphicViewer } from "../viewer/GraphicViewer";
import { getPresentationReferenceHref } from "./presentationAssets";
import { usePresentationAsset } from "./usePresentationAsset";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { SidePanelButton, SidePanelHeader } from "../layout/SidePanelHeader";
import { useModalA11y } from "../../hooks/useModalA11y";
import type { GroupPresentation, LedgerEvent, PresentationMessageRef } from "../../types";
import {
  fetchPresentationBrowserSurfaceSession,
  getGroupBlobUrl,
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
  onClearSlot?: (slotId: string) => void | Promise<void>;
  onSelectSlot?: (slotId: string) => void;
  onPinSlot?: (slotId: string) => void;
  onCollapse?: () => void;
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
  onSelectSlot,
  onPinSlot,
  onCollapse,
  onClose,
}: PresentationViewerProps) {
  const { t, i18n } = useTranslation("chat");
  const showError = useUIStore((state) => state.showError);
  const isModal = variant === "modal";
  const slotButtonRef = useRef<HTMLButtonElement>(null);
  const hasSlotNavigation = !!onSelectSlot;
  const { modalRef } = useModalA11y(isModal && isOpen, onClose, {
    initialFocusRef: hasSlotNavigation ? slotButtonRef : undefined,
  });
  useLayoutEffect(() => {
    if (!isModal && isOpen && hasSlotNavigation) slotButtonRef.current?.focus();
  }, [isModal, isOpen, hasSlotNavigation, groupId, slotId]);
  const [refreshTick, setRefreshTick] = useState(0);
  const [isExpanded, setIsExpanded] = useState(false);
  const [copiedReference, setCopiedReference] = useState(false);
  const [quotePending, setQuotePending] = useState(false);
  const [clearingSlotId, setClearingSlotId] = useState("");
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
    () => getPresentationReferenceHref(groupId, slot, cacheBust),
    [cacheBust, groupId, slot],
  );
  const publishedAt = formatTimestamp(card?.published_at, i18n.language);
  const useSandboxedPreview =
    !!card && card.card_type === "web_preview" && !String(card.content.url || "").trim();
  const cardType = String(card?.card_type || "").trim();
  const cardMode = String(card?.content.mode || "inline").trim();
  const resourceKey = JSON.stringify([
    groupId,
    slotId,
    card?.published_at,
    cardType,
    cardMode,
    card?.content.workspace_rel_path,
    card?.content.url,
  ]);
  const fetchedImage = isWorkspaceLinked && cardType === "image" && !card?.content.url;
  const linkedAsset = usePresentationAsset(
    href,
    resourceKey,
    !isOpen
      ? null
      : fetchedImage
        ? "image"
        : cardType === "markdown" && cardMode !== "inline"
          ? "markdown"
          : null,
  );
  const markdownReady =
    cardType !== "markdown" || cardMode === "inline" || linkedAsset.content !== null;
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
  const { modalRef: snapshotModalRef } = useModalA11y(
    isOpen && snapshotLightboxOpen && !!currentSnapshotUrl,
    () => setSnapshotLightboxOpen(false),
  );
  const prefersInnerViewportScroll =
    cardType === "web_preview" || cardType === "pdf" || cardType === "image";
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
  const iconButtonClassName = `${buttonVariants({ variant: "ghost", size: "iconRail" })} max-sm:h-11 max-sm:w-11`;
  const destructiveIconButtonClassName = `${buttonVariants({ variant: "destructive", size: "iconRail" })} max-sm:h-11 max-sm:w-11`;
  const copiedIconButtonClassName = `${iconButtonClassName} text-[var(--color-accent-success)]`;
  const refreshActionLabel = t("presentationRefreshAction", { defaultValue: "Refresh" });
  const copyActionLabel = copiedReference
    ? t("presentationCopyReferenceCopied", { defaultValue: "Copied" })
    : t("presentationCopyReferenceAction", { defaultValue: "Copy URL/path" });
  const editActionLabel = t("presentationReplaceAction", { defaultValue: "Edit" });
  const clearActionLabel = clearingSlotId
    ? t("presentationClearingAction", { defaultValue: "Clearing..." })
    : t("presentationClearAction", { defaultValue: "Clear" });
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

  const handleClearSlot = async () => {
    if (!slot || !onClearSlot || clearingSlotId) return;
    setClearingSlotId(slot.slot_id);
    try {
      await onClearSlot(slot.slot_id);
    } catch (error) {
      showError(error instanceof Error ? error.message : String(error));
    } finally {
      setClearingSlotId("");
    }
  };

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
    // Documents own their reading/navigation state. Reload them only on an
    // explicit refresh or publication, rather than replacing the iframe every tick.
    if (cardType !== "image" && cardType !== "markdown") return;
    const timer = window.setInterval(() => {
      if (!linkedAsset.refreshing.current) setRefreshTick((value) => value + 1);
    }, 5000);
    return () => window.clearInterval(timer);
  }, [
    isOpen,
    isWorkspaceLinked,
    groupId,
    slotId,
    cardType,
    card?.published_at,
    linkedAsset.refreshing,
  ]);

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
    if (!isOpen || !canRestoreRefInViewer || targetViewerScrollTop == null || !markdownReady)
      return;

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
  }, [canRestoreRefInViewer, isOpen, markdownReady, slotId, targetViewerScrollTop]);

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
        "border-[var(--color-border-primary)] bg-[var(--color-bg-primary)]",
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
            <div className={classNames("mt-1 text-xs", "text-[var(--color-text-tertiary)]")}>
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
          className={iconButtonClassName}
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
        "border-[var(--color-border-primary)] text-[var(--color-text-tertiary)]",
      )}
    >
      {t("presentationMissingCard", { defaultValue: "This presentation slot is empty." })}
    </div>
  ) : card.card_type === "markdown" ? (
    <MarkdownDocumentSurface
      content={String(
        card.content.mode === "inline" ? card.content.markdown || "" : linkedAsset.content || "",
      )}
      error={linkedAsset.stale ? "" : linkedAsset.error}
      loading={cardMode !== "inline" && linkedAsset.content === null && !linkedAsset.error}
      loadingLabel={t("common:loading")}
      isDark={isDark}
      className="!rounded-none !border-0 !bg-transparent !p-3 sm:!p-4"
      minHeightClassName={isModal ? undefined : "min-h-0"}
    />
  ) : card.card_type === "table" ? (
    <div
      className={classNames(
        "overflow-hidden rounded-3xl border",
        "border-[var(--color-border-primary)] bg-[var(--color-bg-primary)]",
      )}
    >
      <div className="overflow-auto">
        <table className="min-w-full border-collapse text-sm">
          <thead className={"bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)]"}>
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
          <tbody className={"text-[var(--color-text-secondary)]"}>
            {(card.content.table?.rows || []).map((row, rowIndex) => (
              <tr
                key={`row-${rowIndex}`}
                className={
                  rowIndex % 2 === 0 ? "bg-[var(--color-bg-primary)]" : "bg-[var(--glass-tab-bg)]"
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
    fetchedImage && linkedAsset.content === null ? (
      <p
        role={linkedAsset.error ? "alert" : "status"}
        className="p-4 text-sm text-[var(--color-text-secondary)]"
      >
        {linkedAsset.error
          ? `${t("imagePreviewUnavailable")} (${linkedAsset.error})`
          : t("common:loading")}
      </p>
    ) : (
      <GraphicViewer
        resourceKey={resourceKey}
        src={fetchedImage ? linkedAsset.content! : href}
        alt={card.title}
      />
    )
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
        "border-[var(--color-border-primary)] bg-[var(--color-bg-primary)]",
      )}
    >
      <div className={classNames("text-base font-semibold", "text-[var(--color-text-primary)]")}>
        {card.title}
      </div>
      <div className={classNames("mt-2 text-sm", "text-[var(--color-text-tertiary)]")}>
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
              {linkedAsset.stale ? (
                <span
                  role="status"
                  className="min-w-0 flex-1 truncate text-amber-700 dark:text-amber-300"
                  title={`${t("presentationRefreshFailed")} (${linkedAsset.error})`}
                >
                  {t("presentationRefreshFailed")}
                </span>
              ) : publishedAt ? (
                <span>{publishedAt}</span>
              ) : null}
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
                        "rounded-full px-2.5 py-1 text-xs font-medium whitespace-nowrap transition-colors",
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
                        "rounded-full px-2.5 py-1 text-xs font-medium whitespace-nowrap transition-colors",
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
                    onClick={() => void handleClearSlot()}
                    disabled={!!clearingSlotId}
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
              ref={snapshotModalRef}
              role="dialog"
              aria-modal="true"
              aria-label={t("presentationSnapshotFromQuoteLabel")}
              className={classNames(
                "relative z-10 flex h-full max-h-full w-full max-w-6xl min-h-0 flex-col overflow-hidden rounded-3xl border shadow-2xl",
                "border-[var(--color-border-primary)] bg-[var(--glass-panel-bg)]",
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
                      "text-[var(--color-text-primary)]",
                    )}
                  >
                    {t("presentationSnapshotFromQuoteLabel", {
                      defaultValue: "Snapshot from this quote",
                    })}
                  </div>
                  {snapshotTimestamp ? (
                    <div
                      className={classNames("mt-1 text-xs", "text-[var(--color-text-tertiary)]")}
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
                  className={iconButtonClassName}
                  aria-label={t("presentationCloseSnapshotAction", {
                    defaultValue: "Close snapshot",
                  })}
                  title={t("presentationCloseSnapshotAction", { defaultValue: "Close snapshot" })}
                >
                  <CloseIcon aria-hidden="true" className="h-4 w-4" strokeWidth={1.6} />
                </button>
              </div>
              <div className="min-h-0 flex-1">
                <GraphicViewer
                  src={currentSnapshotUrl}
                  alt={t("presentationQuotedSnapshotAlt", { defaultValue: "Quoted snapshot" })}
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
                "border-[var(--color-border-primary)] bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)]",
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

  const slotNavigation = onSelectSlot ? (
    <PresentationSlotNavigation
      presentation={presentation}
      activeSlotId={slotId}
      selectedButtonRef={slotButtonRef}
      readOnly={readOnly}
      onSelectSlot={onSelectSlot}
      onPinSlot={onPinSlot}
    />
  ) : null;

  if (variant === "split") {
    return (
      <section
        className={classNames(
          "flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
          "bg-[var(--color-bg-primary)]",
        )}
        aria-label={t("presentationTitle", { defaultValue: "Presentation" })}
      >
        <SidePanelHeader
          title={card?.title || t("presentationTitle")}
          subtitle={
            linkedAsset.stale
              ? t("presentationRefreshFailed")
              : card
                ? getCardTypeLabel(card.card_type, t)
                : undefined
          }
          onClose={onClose}
          closeLabel={t("presentationCloseDockAction")}
        >
          {onCollapse && (
            <SidePanelButton title={t("presentationCompactSlots")} onClick={onCollapse}>
              <PanelRightClose />
            </SidePanelButton>
          )}
          {onOpenWindow && (
            <SidePanelButton title={t("presentationOpenWindowAction")} onClick={onOpenWindow}>
              <WindowViewIcon />
            </SidePanelButton>
          )}
        </SidePanelHeader>
        {linkedAsset.stale && (
          <span role="status" className="sr-only">
            {t("presentationRefreshFailed")}
          </span>
        )}
        {slotNavigation}
        {(showWebPreviewModeToggle ||
          canRefresh ||
          copyReferenceValue ||
          (!readOnly && (onReplaceSlot || onClearSlot) && slot)) && (
          <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-[var(--glass-border-subtle)] px-2 py-1">
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
                    "rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-colors",
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
                    "rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-colors",
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
            <div className="ml-auto flex shrink-0 items-center gap-1">
              {canRefresh && (
                <SidePanelButton
                  title={refreshActionLabel}
                  onClick={() => setRefreshTick((value) => value + 1)}
                >
                  <RefreshIcon />
                </SidePanelButton>
              )}
              {copyReferenceValue && (
                <SidePanelButton title={copyActionLabel} onClick={() => void handleCopyReference()}>
                  <CopyIcon />
                </SidePanelButton>
              )}
              {!readOnly && onReplaceSlot && slot && (
                <SidePanelButton
                  title={editActionLabel}
                  onClick={() => onReplaceSlot(slot.slot_id)}
                >
                  <EditIcon />
                </SidePanelButton>
              )}
              {!readOnly && onClearSlot && slot && (
                <SidePanelButton
                  title={clearActionLabel}
                  onClick={() => void handleClearSlot()}
                  disabled={!!clearingSlotId}
                  className={
                    isDark ? "text-rose-200 hover:bg-rose-500/15" : "text-rose-700 hover:bg-rose-50"
                  }
                >
                  <TrashIcon />
                </SidePanelButton>
              )}
            </div>
          </div>
        )}
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
      {slotNavigation}
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
