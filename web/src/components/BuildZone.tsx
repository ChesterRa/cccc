// BuildZone: renders a grid of BuildSites, one per active task
// Each plot independently resolves its blueprint via useBlueprintResolver

import { useMemo } from "react";
import { BuildSite, type BuildStatus } from "./BuildSite";
import { useBlueprintResolver } from "../hooks/useBlueprintResolver";
import { computeGridPosition } from "../utils/buildLayout";
import type { Task } from "../types";

interface BuildZoneProps {
  tasks: Task[];
  baseZ: number;
  isDark: boolean;
}

/** Derive progress (0-1) and build status from a Task */
function taskProgress(task: Task): { progress: number; status: BuildStatus } {
  if (task.status === "done" || task.status === "archived") {
    return { progress: 1, status: "complete" };
  }

  // Use explicit progress field if available
  if (task.progress != null && task.progress > 0) {
    return { progress: Math.min(task.progress, 1), status: "building" };
  }

  if (task.steps && task.steps.length > 0) {
    const done = task.steps.filter((s) => s.status === "done").length;
    const progress = done / task.steps.length;
    return { progress, status: progress > 0 ? "building" : "ghost" };
  }

  if (task.status === "in_progress") {
    return { progress: 0.3, status: "building" };
  }

  return { progress: 0, status: "ghost" };
}

/** Single build plot: resolves blueprint + renders BuildSite */
function BuildPlot({
  task,
  position,
  isDark,
}: {
  task: Task;
  position: [number, number, number];
  isDark: boolean;
}) {
  const { blueprint } = useBlueprintResolver(task);
  const { progress, status } = taskProgress(task);

  if (!blueprint) return null;

  return (
    <BuildSite
      blueprint={blueprint}
      progress={progress}
      status={status}
      position={position}
      label={task.name.replace(/^T\d+:\s*/, "")}
      isDark={isDark}
    />
  );
}

export function BuildZone({ tasks, baseZ, isDark }: BuildZoneProps) {
  const taskCount = tasks.length;
  const positions = useMemo(
    () => Array.from({ length: taskCount }, (_, i) => computeGridPosition(i, taskCount, baseZ)),
    [taskCount, baseZ],
  );

  if (tasks.length === 0) return null;

  return (
    <group>
      {tasks.map((task, i) => (
        <BuildPlot
          key={task.id}
          task={task}
          position={positions[i]}
          isDark={isDark}
        />
      ))}
    </group>
  );
}
