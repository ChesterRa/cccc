import { lazy, Suspense } from "react";
import type { AgentState, Actor, Task } from "../types";

const ActorScene3D = lazy(() =>
  import("../components/ActorScene3D").then((m) => ({ default: m.ActorScene3D }))
);

interface PanoramaTabProps {
  agents: AgentState[];
  actors?: Actor[];
  tasks?: Task[];
  isDark: boolean;
  groupId?: string;
}

export function PanoramaTab({ agents, actors, tasks, isDark, groupId }: PanoramaTabProps) {
  if (agents.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <span className={isDark ? "text-slate-500 text-sm" : "text-gray-400 text-sm"}>
          No agents online
        </span>
      </div>
    );
  }

  return (
    <Suspense
      fallback={
        <div className="flex-1 flex items-center justify-center">
          <span className={isDark ? "text-slate-500 text-xs" : "text-gray-400 text-xs"}>
            Loading 3D scene...
          </span>
        </div>
      }
    >
      <ActorScene3D agents={agents} actors={actors} tasks={tasks} isDark={isDark} groupId={groupId} className="flex-1" />
    </Suspense>
  );
}
