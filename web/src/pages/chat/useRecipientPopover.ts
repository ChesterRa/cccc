import type { CSSProperties } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import type { Actor } from "../../types";
import { formatRecipientIdentifier } from "../../utils/recipientIdentifier";

export interface RecipientPopoverTarget {
  key: string;
  label: string;
  kindLabel: string;
  badgeLabel?: string;
  identifier: string;
  idValue?: string;
}

export const RECIPIENT_POPOVER_GAP_PX = 6;

export function useRecipientPopover({ isSmallScreen }: { isSmallScreen: boolean }) {
  const { t } = useTranslation("chat");
  const copyWithFeedback = useCopyFeedback();
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [target, setTarget] = useState<RecipientPopoverTarget | null>(null);
  const [style, setStyle] = useState<CSSProperties | null>(null);

  const cancelHide = useCallback(() => {
    if (!hideTimerRef.current) return;
    clearTimeout(hideTimerRef.current);
    hideTimerRef.current = null;
  }, []);

  useEffect(() => () => cancelHide(), [cancelHide]);

  const show = useCallback(
    (nextTarget: RecipientPopoverTarget, node: HTMLElement) => {
      cancelHide();
      const rect = node.getBoundingClientRect();
      const viewportWidth = typeof window === "undefined" ? 1024 : window.innerWidth;
      const tooltipWidth = Math.min(196, Math.max(176, viewportWidth - 16));
      const top = rect.top;
      const transform = `translateY(calc(-100% - ${RECIPIENT_POPOVER_GAP_PX}px))`;
      if (isSmallScreen) {
        setStyle({ top, left: 8, right: 8, transform });
      } else {
        setStyle({
          top,
          left: Math.min(Math.max(rect.left, 8), Math.max(8, viewportWidth - tooltipWidth - 8)),
          width: tooltipWidth,
          transform,
        });
      }
      setTarget(nextTarget);
    },
    [cancelHide, isSmallScreen],
  );

  const hide = useCallback(() => {
    cancelHide();
    setTarget(null);
    setStyle(null);
  }, [cancelHide]);

  const scheduleHide = useCallback(() => {
    cancelHide();
    hideTimerRef.current = setTimeout(() => {
      setTarget(null);
      setStyle(null);
      hideTimerRef.current = null;
    }, 120);
  }, [cancelHide]);

  const copyIdentifier = useCallback(
    async (identifier: string) => {
      const text = String(identifier || "").trim();
      if (!text) return;
      await copyWithFeedback(text, {
        successMessage: t("recipientIdentifierCopied", {
          defaultValue: "Recipient identifier copied.",
        }),
        errorMessage: t("common:copyFailed", { defaultValue: "Copy failed." }),
      });
    },
    [copyWithFeedback, t],
  );

  const selectorTarget = useCallback(
    (selector: string): RecipientPopoverTarget => ({
      key: `selector:${selector}`,
      label: selector,
      kindLabel: t("recipientSelectorDetail", { defaultValue: "Local selector" }),
      identifier: formatRecipientIdentifier({ kind: "selector", selector }),
    }),
    [t],
  );

  const actorTarget = useCallback(
    (actor: Actor): RecipientPopoverTarget => {
      const id = String(actor.id || "").trim();
      const label = String(actor.title || id || "actor").trim();
      const role = String(actor.role || "").trim();
      return {
        key: `actor:${id || label}`,
        label,
        kindLabel: t("recipientActorDetail", { defaultValue: "Local actor" }),
        badgeLabel: role || undefined,
        identifier: formatRecipientIdentifier({ kind: "actor", label, id, role }),
        idValue: id,
      };
    },
    [t],
  );

  return {
    visibleTarget: target,
    style,
    show,
    hide,
    cancelHide,
    scheduleHide,
    copyIdentifier,
    selectorTarget,
    actorTarget,
  };
}
