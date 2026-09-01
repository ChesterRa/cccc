import type { ReactNode } from "react";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "./ModalFrame";

export function RuntimeInspectorModal({
  isOpen,
  isDark,
  onClose,
  titleId,
  closeAriaLabel,
  children,
}: {
  isOpen: boolean;
  isDark: boolean;
  onClose: () => void;
  titleId: string;
  closeAriaLabel: string;
  children: ReactNode;
}) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId={titleId}
      title=""
      closeAriaLabel={closeAriaLabel}
      panelClassName="h-full w-full max-w-none overflow-hidden sm:h-[92vh] sm:w-[min(1480px,98vw)] sm:max-w-[98vw]"
      floatingCloseClassName="sm:!top-3"
      floatingCloseButtonClassName="!min-h-[40px] !min-w-[40px] !rounded-xl"
      modalRef={modalRef}
    >
      {children}
    </ModalFrame>
  );
}
