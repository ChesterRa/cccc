import { useId, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useTheme } from "../../hooks/useTheme";
import { SessionNodeHubLabel } from "./SessionNodeProgress";
import { SessionNodeJoinReview } from "./SessionNodeJoinReview";

export function SessionNodeJoinDialog({
  requestId,
  onClose,
  onChooseSessions,
}: {
  requestId: string;
  onClose: () => void;
  onChooseSessions: () => void;
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
        <div className="min-w-0">
          <SessionNodeHubLabel />
          <h2 ref={title} tabIndex={-1} className="text-2xl font-semibold outline-none">
            {t("localSessions.addDevice")}
          </h2>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="self-center w-[calc(100vw_-_1rem)] max-w-[554px] max-h-[calc(100dvh_-_1rem)] rounded-xl sm:max-h-[88dvh]"
    >
      <div className="min-h-0 overflow-y-auto px-4 pt-3 pb-4 sm:px-6 sm:pb-5">
        <SessionNodeJoinReview
          key={requestId}
          requestId={requestId}
          onChooseSessions={onChooseSessions}
        />
      </div>
    </ModalFrame>,
    document.body,
  );
}
