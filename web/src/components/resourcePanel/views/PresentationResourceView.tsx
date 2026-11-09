import { useEffect, useLayoutEffect, useRef, useState, type ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { PresentationViewerSplitPanel } from "../../presentation/PresentationViewerModal";
import { useModalStore } from "../../../stores/useModalStore";
import { fetchMessageWindow } from "../../../services/api";
import { getPresentationMessageRefs } from "../../../utils/presentationRefs";
import { presentationReferenceIndex } from "../../../stores/resourcePanelPersistence";
import type { PresentationMessageRef } from "../../../types";
import { buttonVariants } from "../../ui/button-variants";

type ViewerProps = ComponentProps<typeof PresentationViewerSplitPanel>;

export type PresentationResourceHandlers = Pick<
  ViewerProps,
  | "onQuoteInChat"
  | "onOpenMessageContext"
  | "onReplyToMessage"
  | "onReplaceSlot"
  | "onClearSlot"
  | "onSelectSlot"
  | "onPinSlot"
  | "onOpenWindow"
>;

type PresentationResourceViewProps = Pick<
  ViewerProps,
  | "groupId"
  | "slotId"
  | "presentation"
  | "sourceEvent"
  | "focusRef"
  | "focusEventId"
  | "isDark"
  | "readOnly"
> & { visible: boolean; handlers: PresentationResourceHandlers; referenceKey?: string };

/** The surrounding panel owns closing; the slot and its shared session remain independent. */
export function PresentationResourceView({
  handlers,
  referenceKey = "live",
  ...props
}: PresentationResourceViewProps) {
  const { t } = useTranslation("chat");
  const needsReference = referenceKey !== "live" && !props.focusRef;
  const identity = JSON.stringify([props.groupId, props.slotId, props.focusEventId, referenceKey]);
  const [restored, setRestored] = useState<{
    identity: string;
    ref: PresentationMessageRef | null;
    error?: string;
  } | null>(null);
  const referenceLoaded = restored?.identity === identity;
  useEffect(() => {
    if (!needsReference || !props.visible || referenceLoaded) return;
    let current = true;
    const eventId = props.focusEventId || "";
    const index = presentationReferenceIndex(eventId, referenceKey);
    if (index === null) {
      setRestored({ identity, ref: null });
      return;
    }
    void fetchMessageWindow(props.groupId, eventId, { before: 0, after: 0 }).then(
      (response) => {
        if (!current) return;
        if (!response.ok) {
          setRestored({
            identity,
            ref: null,
            error: response.error.message || t("resourcePanel.referenceLoadFailed"),
          });
          return;
        }
        const event = response.result.events.find((event) => event.id === eventId);
        const refs = event?.data && "refs" in event.data ? event.data.refs : undefined;
        const ref = getPresentationMessageRefs(refs)[index];
        setRestored({ identity, ref: ref?.slot_id === props.slotId ? ref : null });
      },
      () => {
        if (current)
          setRestored({ identity, ref: null, error: t("resourcePanel.referenceLoadFailed") });
      },
    );
    return () => {
      current = false;
    };
  }, [
    identity,
    needsReference,
    props.focusEventId,
    props.groupId,
    props.slotId,
    props.visible,
    referenceKey,
    referenceLoaded,
    t,
  ]);
  const reference = props.focusRef ?? (restored?.identity === identity ? restored.ref : null);
  const ready = !needsReference || !!reference;
  const visibilityOwner = useRef(Symbol("presentation resource"));
  const setVisible = useModalStore((state) => state.setVisiblePresentationResource);
  useLayoutEffect(() => {
    if (!props.visible || !ready) return undefined;
    const owner = visibilityOwner.current;
    setVisible(owner, { groupId: props.groupId, slotId: props.slotId });
    return () => setVisible(owner, null);
  }, [props.groupId, props.slotId, props.visible, ready, setVisible]);
  if (needsReference && !ready)
    return (
      <div
        role={referenceLoaded ? "alert" : "status"}
        className="flex flex-col items-start gap-3 p-4 text-sm text-[var(--color-text-secondary)]"
      >
        <p>
          {referenceLoaded && restored?.error
            ? restored.error
            : t(referenceLoaded ? "resourcePanel.referenceUnavailable" : "common:loading")}
        </p>
        {referenceLoaded && restored?.error ? (
          <button
            type="button"
            className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
            data-lcd-key
            onClick={() => setRestored(null)}
          >
            {t("common:retry")}
          </button>
        ) : null}
      </div>
    );
  return (
    <PresentationViewerSplitPanel
      {...props}
      {...handlers}
      focusRef={reference}
      embedded
      reuseExistingSession
      onClose={() => undefined}
    />
  );
}
