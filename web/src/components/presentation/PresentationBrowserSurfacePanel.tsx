import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchPresentationBrowserSurfaceSession,
  getPresentationBrowserSurfaceWebSocketUrl,
  startPresentationBrowserSurfaceSession,
} from "../../services/api";
import {
  ProjectedBrowserSurfacePanel,
  type ProjectedBrowserFrame,
} from "../browser/ProjectedBrowserSurfacePanel";
import { useGroupStore } from "../../stores/useGroupStore";
import { findPresentationSlot } from "../../utils/presentation";
import { getPresentationReferenceHref } from "./presentationAssets";

type PresentationBrowserSurfacePanelProps = {
  groupId: string;
  slotId: string;
  url: string;
  isDark: boolean;
  refreshNonce: number;
  visible?: boolean;
  reuseExistingSession?: boolean;
  viewportClassName?: string;
  onFrameUpdate?: (frame: ProjectedBrowserFrame | null) => void;
};

export type PresentationBrowserFrame = ProjectedBrowserFrame;

export function PresentationBrowserSurfacePanel({
  groupId,
  slotId,
  url,
  isDark,
  refreshNonce,
  visible = true,
  reuseExistingSession = false,
  viewportClassName,
  onFrameUpdate,
}: PresentationBrowserSurfacePanelProps) {
  const { t } = useTranslation("chat");
  const [resolution, setResolution] = useState<"checking" | "confirm" | "failed" | null>(null);
  const [replaceRequested, setReplaceRequested] = useState(false);
  const checkGeneration = useRef(0);
  useEffect(
    () => () => {
      checkGeneration.current += 1;
    },
    [],
  );

  const checkCurrentCard = useCallback(
    async (replace: boolean) => {
      const generation = ++checkGeneration.current;
      setResolution("checking");
      const refreshed = await useGroupStore.getState().refreshPresentation(groupId);
      if (generation !== checkGeneration.current) return;
      if (!refreshed) {
        setResolution("failed");
        return;
      }
      const state = useGroupStore.getState();
      if (state.selectedGroupId !== groupId) return;
      const slot = findPresentationSlot(state.groupPresentation, slotId);
      // A newer card causes the parent viewer to replace this component using its new URL.
      if (
        slot?.card?.card_type !== "web_preview" ||
        getPresentationReferenceHref(groupId, slot) !== url
      )
        return;
      const current = await fetchPresentationBrowserSurfaceSession(groupId, slotId);
      if (generation !== checkGeneration.current) return;
      if (!current.ok) {
        setResolution("failed");
        return;
      }
      const session = current.result.browser_surface;
      const differs = session.active && !!session.initial_url && session.initial_url !== url;
      if (differs && !replace) {
        setResolution("confirm");
        return;
      }
      // Recheck on click: another viewer may already have opened the desired card.
      setReplaceRequested(!!differs && replace);
      setResolution(null);
    },
    [groupId, slotId, url],
  );

  if (resolution) {
    return (
      <div
        role={resolution === "checking" ? "status" : "alert"}
        className="flex h-full flex-col items-start justify-center gap-3 p-4 text-sm"
      >
        <p>
          {t(
            resolution === "checking"
              ? "presentationBrowserStarting"
              : resolution === "failed"
                ? "presentationBrowserRefreshFailed"
                : "presentationBrowserContentChanged",
          )}
        </p>
        {resolution !== "checking" ? (
          <button
            type="button"
            className="rounded-md border border-[var(--color-border-primary)] px-3 py-2 hover:bg-[var(--glass-tab-bg)]"
            onClick={() => void checkCurrentCard(resolution === "confirm")}
          >
            {t(resolution === "failed" ? "common:retry" : "presentationBrowserOpenNewContent")}
          </button>
        ) : null}
      </div>
    );
  }
  return (
    <ProjectedBrowserSurfacePanel
      isDark={isDark}
      refreshNonce={refreshNonce}
      visible={visible}
      reuseActiveSession={reuseExistingSession}
      expectedInitialUrl={reuseExistingSession ? url : undefined}
      onContentChanged={() => void checkCurrentCard(false)}
      replaceOnStart={replaceRequested}
      sessionIdentity={`${groupId}:${slotId}:${url}`}
      defaultViewerMode="page"
      chromeMode="embedded"
      viewportClassName={viewportClassName}
      onFrameUpdate={onFrameUpdate}
      fallbackUrl={url}
      webSocketUrl={getPresentationBrowserSurfaceWebSocketUrl(groupId, slotId)}
      loadSession={() => fetchPresentationBrowserSurfaceSession(groupId, slotId)}
      startSession={({ width, height, replaceExisting }) =>
        startPresentationBrowserSurfaceSession(groupId, {
          slotId,
          url,
          width,
          height,
          reuseExistingSession: reuseExistingSession && !replaceExisting,
        })
      }
    />
  );
}
