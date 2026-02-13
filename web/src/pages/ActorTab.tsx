import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { Actor, PresenceAgent } from "../types";

const LazyAgentTab = lazy(() => import("../components/AgentTab").then((m) => ({ default: m.AgentTab })));

export interface ActorTabProps {
  actor: Actor | null;
  groupId: string;
  presenceAgent: PresenceAgent | null;
  termEpoch: number;
  busy: string;
  isDark: boolean;
  isSmallScreen: boolean;
  isVisible: boolean;
  readOnly?: boolean;
  onToggleEnabled: () => void;
  onRelaunch: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onInbox: () => void;
  /** Called when actor status may have changed (e.g., process exited) */
  onStatusChange?: () => void;
}

export function ActorTab({
  actor,
  groupId,
  presenceAgent,
  termEpoch,
  busy,
  isDark,
  isSmallScreen,
  isVisible,
  readOnly,
  onToggleEnabled,
  onRelaunch,
  onEdit,
  onRemove,
  onInbox,
  onStatusChange,
}: ActorTabProps) {
  const { t } = useTranslation('actors');

  if (!actor) {
    return <div className="flex-1 flex items-center justify-center text-slate-500">{t('agentNotFound')}</div>;
  }

  return (
    <Suspense
      fallback={<div className={`flex-1 flex items-center justify-center ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('loadingAgent')}</div>}
    >
      <LazyAgentTab
        key={`${groupId}:${actor.id}:${termEpoch}`}
        actor={actor}
        groupId={groupId}
        presenceAgent={presenceAgent}
        isVisible={isVisible}
        readOnly={readOnly}
        onQuit={onToggleEnabled}
        onLaunch={onToggleEnabled}
        onRelaunch={onRelaunch}
        onEdit={onEdit}
        onRemove={onRemove}
        onInbox={onInbox}
        busy={busy}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        onStatusChange={onStatusChange}
      />
    </Suspense>
  );
}
