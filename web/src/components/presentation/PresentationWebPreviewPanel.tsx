import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { PresentationBrowserSurfacePanel } from "./PresentationBrowserSurfacePanel";
import type { PresentationBrowserFrame } from "./PresentationBrowserSurfacePanel";

export type PresentationWebPreviewMode = "embedded" | "interactive";

type PresentationWebPreviewPanelProps = {
  groupId: string;
  slotId: string;
  title: string;
  href: string;
  isDark: boolean;
  useSandboxedPreview: boolean;
  allowLiveBrowser: boolean;
  mode: PresentationWebPreviewMode;
  refreshNonce: number;
  viewportClassName?: string;
  onInteractiveFrameUpdate?: (frame: PresentationBrowserFrame | null) => void;
};

export function PresentationWebPreviewPanel({
  groupId,
  slotId,
  title,
  href,
  isDark,
  useSandboxedPreview,
  allowLiveBrowser,
  mode,
  refreshNonce,
  viewportClassName,
  onInteractiveFrameUpdate,
}: PresentationWebPreviewPanelProps) {
  const { t } = useTranslation("chat");
  const effectiveMode: PresentationWebPreviewMode = allowLiveBrowser ? mode : "embedded";

  return (
    <div className="flex h-full min-h-0 flex-col">
      {effectiveMode === "interactive" ? (
        <PresentationBrowserSurfacePanel
          groupId={groupId}
          slotId={slotId}
          url={href}
          isDark={isDark}
          refreshNonce={refreshNonce}
          viewportClassName={viewportClassName}
          onFrameUpdate={onInteractiveFrameUpdate}
        />
      ) : (
        <iframe
          key={`embedded:${href}:${refreshNonce}`}
          title={title || t("presentationTypeWebPreview", { defaultValue: "Web" })}
          src={href}
          sandbox={
            useSandboxedPreview
              ? "allow-scripts allow-forms allow-modals allow-popups allow-downloads"
              : undefined
          }
          className={classNames(
            viewportClassName || "flex-1 min-h-0",
            "w-full flex-1 min-h-0 rounded-3xl border border-[var(--glass-border-subtle)] bg-white",
          )}
        />
      )}
    </div>
  );
}
