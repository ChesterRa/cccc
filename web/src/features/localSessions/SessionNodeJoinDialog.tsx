import { useId, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useTheme } from "../../hooks/useTheme";
import { SessionNodeJoinReview } from "./SessionNodeJoinReview";

export function SessionNodeJoinDialog({
  requestId,
  onClose,
}: {
  requestId: string;
  onClose: () => void;
}) {
  const { t } = useTranslation("layout");
  const { isDark } = useTheme();
  const title = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  const { modalRef } = useModalA11y(true, onClose, { initialFocusRef: title });
  return createPortal(
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      modalRef={modalRef}
      surface="solid"
      titleId={titleId}
      title={
        <h2 ref={title} tabIndex={-1} className="text-lg font-semibold outline-none">
          {t("sessionNodes.joinReviewTitle")}
        </h2>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="w-full self-center max-w-[calc(100vw_-_2rem)] sm:max-w-md max-h-[88dvh]"
    >
      <div className="min-h-0 overflow-y-auto p-4 sm:p-6">
        <SessionNodeJoinReview key={requestId} requestId={requestId} />
      </div>
    </ModalFrame>,
    document.body,
  );
}
