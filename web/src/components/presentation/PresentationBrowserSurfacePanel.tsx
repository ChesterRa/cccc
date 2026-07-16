import {
  fetchPresentationBrowserSurfaceSession,
  getPresentationBrowserSurfaceWebSocketUrl,
  startPresentationBrowserSurfaceSession,
} from "../../services/api";
import {
  ProjectedBrowserSurfacePanel,
  type ProjectedBrowserFrame,
} from "../browser/ProjectedBrowserSurfacePanel";

type PresentationBrowserSurfacePanelProps = {
  groupId: string;
  slotId: string;
  url: string;
  isDark: boolean;
  refreshNonce: number;
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
  viewportClassName,
  onFrameUpdate,
}: PresentationBrowserSurfacePanelProps) {
  return (
    <ProjectedBrowserSurfacePanel
      isDark={isDark}
      refreshNonce={refreshNonce}
      chromeMode="embedded"
      viewportClassName={viewportClassName}
      onFrameUpdate={onFrameUpdate}
      fallbackUrl={url}
      webSocketUrl={getPresentationBrowserSurfaceWebSocketUrl(groupId, slotId)}
      loadSession={() => fetchPresentationBrowserSurfaceSession(groupId, slotId)}
      startSession={({ width, height }) =>
        startPresentationBrowserSurfaceSession(groupId, { slotId, url, width, height })
      }
    />
  );
}
