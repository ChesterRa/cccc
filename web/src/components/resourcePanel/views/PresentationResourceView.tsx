import { useLayoutEffect, useRef, type ComponentProps } from "react";
import { PresentationViewerSplitPanel } from "../../presentation/PresentationViewerModal";
import { useModalStore } from "../../../stores/useModalStore";

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
> & { visible: boolean; handlers: PresentationResourceHandlers };

/** The surrounding panel owns closing; the slot and its shared session remain independent. */
export function PresentationResourceView({ handlers, ...props }: PresentationResourceViewProps) {
  const visibilityOwner = useRef(Symbol("presentation resource"));
  const setVisible = useModalStore((state) => state.setVisiblePresentationResource);
  useLayoutEffect(() => {
    if (!props.visible) return undefined;
    const owner = visibilityOwner.current;
    setVisible(owner, { groupId: props.groupId, slotId: props.slotId });
    return () => setVisible(owner, null);
  }, [props.groupId, props.slotId, props.visible, setVisible]);
  return (
    <PresentationViewerSplitPanel
      {...props}
      {...handlers}
      embedded
      reuseExistingSession
      onClose={() => undefined}
    />
  );
}
