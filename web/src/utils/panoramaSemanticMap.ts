import type { BedInstance } from "../components/MCFurniture";
import type { LayoutItem } from "../hooks/useCharacterAnimation";
import type { Actor, AgentState, Task } from "../types";
import { deriveAnimState, type AgentAnimState } from "./actorUtils";

export type SemanticZoneKind = "foreman" | "blocked" | "task" | "idle" | "offline";

export interface SemanticZone {
  id: string;
  kind: SemanticZoneKind;
  label: string;
  subtitle?: string;
  color: string;
  center: [number, number, number];
  size: [number, number];
  agentIds: string[];
  taskId?: string;
}

export interface SemanticMapState {
  zones: SemanticZone[];
  layout: Map<string, LayoutItem>;
  buildTargetMap: Map<string, [number, number, number]>;
  bedInstances: BedInstance[];
}

export const SEMANTIC_MAP_SCENE_EXTENT = 9;

const TASK_ZONE_COLORS = ["#2563eb", "#0f766e", "#7c3aed"];

function agentActiveTaskId(agent: AgentState): string {
  return String(agent.hot?.active_task_id || "").trim();
}

function taskLabel(task: Task): string {
  return String(task.title || task.id || "未命名任务").trim();
}

function isRunningActor(actor?: Actor): boolean {
  return actor?.running !== false && actor?.enabled !== false;
}

function isDoneStatus(status?: string): boolean {
  return status === "done" || status === "archived";
}

function faceTargetY(from: [number, number, number], to: [number, number, number]): number {
  return Math.atan2(to[0] - from[0], to[2] - from[2]) + Math.PI;
}

function deriveAgentState(agent: AgentState, actor: Actor | undefined, taskStatus?: string): AgentAnimState {
  const idleSeconds = actor?.idle_seconds;
  const lastActivity = idleSeconds != null ? Date.now() - idleSeconds * 1000 : undefined;
  return deriveAnimState(agent, isRunningActor(actor), lastActivity, taskStatus);
}

function gridPositions(center: [number, number, number], size: [number, number], count: number, zBias = 0): Array<[number, number, number]> {
  if (count <= 0) return [];
  const [width, depth] = size;
  const cols = Math.max(1, Math.min(4, Math.ceil(Math.sqrt(count))));
  const rows = Math.max(1, Math.ceil(count / cols));
  const usableW = Math.max(0.9, width - 1.2);
  const usableD = Math.max(0.9, depth - 1.2);
  const stepX = cols > 1 ? usableW / (cols - 1) : 0;
  const stepZ = rows > 1 ? usableD / (rows - 1) : 0;
  const startX = center[0] - usableW / 2;
  const startZ = center[2] - usableD / 2 + zBias;
  const positions: Array<[number, number, number]> = [];

  for (let index = 0; index < count; index++) {
    const col = cols === 1 ? 0 : index % cols;
    const row = rows === 1 ? 0 : Math.floor(index / cols);
    const x = cols === 1 ? center[0] : startX + stepX * col;
    const z = rows === 1 ? center[2] + zBias : startZ + stepZ * row;
    positions.push([x, 0, z]);
  }

  return positions;
}

function bedPositions(zone: SemanticZone, count: number): BedInstance[] {
  if (count <= 0) return [];
  const [width, depth] = zone.size;
  const usableW = Math.max(1.2, width - 1.4);
  const stepX = count > 1 ? usableW / (count - 1) : 0;
  const startX = zone.center[0] - usableW / 2;
  const z = zone.center[2] - depth / 2 + 0.75;

  return Array.from({ length: count }, (_, index) => ({
    position: [count === 1 ? zone.center[0] : startX + stepX * index, 0, z] as [number, number, number],
    rotationY: 0,
  }));
}

function compressTaskGroups(taskGroups: Map<string, string[]>, taskMap: Map<string, Task>): Array<{ taskId: string; label: string; agentIds: string[] }> {
  const entries = [...taskGroups.entries()]
    .map(([taskId, agentIds]) => ({
      taskId,
      label: taskLabel(taskMap.get(taskId) || { id: taskId }),
      agentIds: [...agentIds],
    }))
    .sort((left, right) => right.agentIds.length - left.agentIds.length || left.label.localeCompare(right.label));

  if (entries.length <= 3) return entries;

  const primary = entries.slice(0, 2);
  const mergedIds = entries.slice(2).flatMap((item) => item.agentIds);
  primary.push({
    taskId: "__other_tasks__",
    label: "其他任务",
    agentIds: mergedIds,
  });
  return primary;
}

export function buildSemanticMapState(
  agents: AgentState[],
  actorMap: Map<string, Actor>,
  tasks?: Task[],
): SemanticMapState {
  const taskMap = new Map<string, Task>();
  const taskStatusMap = new Map<string, string>();
  for (const task of tasks || []) {
    taskMap.set(task.id, task);
    if (task.status) taskStatusMap.set(task.id, task.status);
  }

  const derivedStateById = new Map<string, AgentAnimState>();
  for (const agent of agents) {
    const actor = actorMap.get(agent.id);
    const activeTaskId = agentActiveTaskId(agent);
    const taskStatus = activeTaskId ? taskStatusMap.get(activeTaskId) : undefined;
    derivedStateById.set(agent.id, deriveAgentState(agent, actor, taskStatus));
  }

  const foremanIds: string[] = [];
  const blockedIds: string[] = [];
  const idleIds: string[] = [];
  const offlineIds: string[] = [];
  const taskGroups = new Map<string, string[]>();

  for (const agent of agents) {
    const actor = actorMap.get(agent.id);
    const derived = derivedStateById.get(agent.id) || "idle";
    const activeTaskId = agentActiveTaskId(agent);
    const activeTaskStatus = activeTaskId ? taskStatusMap.get(activeTaskId) : undefined;
    const hasLiveTask = !!(activeTaskId && !isDoneStatus(activeTaskStatus));
    const isForeman = actor?.role === "foreman";

    if (derived === "offline") {
      offlineIds.push(agent.id);
      continue;
    }

    if (derived === "blocked") {
      blockedIds.push(agent.id);
      continue;
    }

    if (isForeman && derived === "working") {
      foremanIds.push(agent.id);
      continue;
    }

    if (isForeman && hasLiveTask && (derived === "working" || derived === "thinking")) {
      foremanIds.push(agent.id);
      continue;
    }

    if (!isForeman && hasLiveTask && (derived === "working" || derived === "thinking")) {
      const bucket = taskGroups.get(activeTaskId) || [];
      bucket.push(agent.id);
      taskGroups.set(activeTaskId, bucket);
      continue;
    }

    idleIds.push(agent.id);
  }

  const compressedTasks = compressTaskGroups(taskGroups, taskMap);
  const taskZoneWidth = compressedTasks.length <= 1 ? 10.2 : compressedTasks.length === 2 ? 6.8 : 4.5;
  const taskCenters = compressedTasks.length <= 1
    ? [0]
    : compressedTasks.length === 2
      ? [-3.8, 3.8]
      : [-4.8, 0, 4.8];

  const zones: SemanticZone[] = [
    {
      id: "blocked",
      kind: "blocked",
      label: "受阻区",
      subtitle: `${blockedIds.length} 人`,
      color: "#b91c1c",
      center: [-4.8, 0, 4.9],
      size: [4.6, 2.9],
      agentIds: blockedIds,
    },
    {
      id: "foreman",
      kind: "foreman",
      label: "指挥区",
      subtitle: `${foremanIds.length} 人`,
      color: "#c2410c",
      center: [4.8, 0, 4.9],
      size: [4.6, 2.9],
      agentIds: foremanIds,
    },
    ...compressedTasks.map((taskGroup, index) => ({
      id: `task:${taskGroup.taskId}`,
      kind: "task" as const,
      label: taskGroup.label,
      subtitle: `${taskGroup.agentIds.length} 人`,
      color: TASK_ZONE_COLORS[index % TASK_ZONE_COLORS.length],
      center: [taskCenters[index] ?? 0, 0, 0] as [number, number, number],
      size: [taskZoneWidth, 4.6] as [number, number],
      agentIds: taskGroup.agentIds,
      taskId: taskGroup.taskId,
    })),
    {
      id: "idle",
      kind: "idle",
      label: "休闲区",
      subtitle: `${idleIds.length} 人`,
      color: "#0f766e",
      center: [-4.8, 0, -4.4],
      size: [4.4, 2.9],
      agentIds: idleIds,
    },
    {
      id: "offline",
      kind: "offline",
      label: "离线区",
      subtitle: `${offlineIds.length} 人`,
      color: "#475569",
      center: [4.8, 0, -4.4],
      size: [4.4, 2.9],
      agentIds: offlineIds,
    },
  ];

  const layout = new Map<string, LayoutItem>();
  const buildTargetMap = new Map<string, [number, number, number]>();
  const offlineZone = zones.find((zone) => zone.kind === "offline");
  const offlineBedCount = offlineZone ? Math.max(2, offlineZone.agentIds.length) : 0;
  const offlineBeds = offlineZone ? bedPositions(offlineZone, offlineBedCount) : [];
  const offlineBedByAgent = new Map<string, BedInstance>();
  offlineZone?.agentIds.forEach((agentId, index) => {
    const bed = offlineBeds[index];
    if (bed) offlineBedByAgent.set(agentId, bed);
  });

  for (const zone of zones) {
    const zBias = zone.kind === "idle" ? 0.5 : 0;
    const positions = gridPositions(zone.center, zone.size, zone.agentIds.length, zBias);
    zone.agentIds.forEach((agentId, index) => {
      const charPos = positions[index] || zone.center;
      const offlineBed = offlineBedByAgent.get(agentId);
      const bedPos = offlineBed?.position || charPos;
      const bedRotY = offlineBed?.rotationY || 0;
      const facingTarget = zone.kind === "task" || zone.kind === "blocked" || zone.kind === "foreman"
        ? zone.center
        : ([0, 0, 0] as [number, number, number]);

      layout.set(agentId, {
        agentId,
        charPos,
        charRotY: faceTargetY(charPos, facingTarget),
        bedPos,
        bedRotY,
        isForeman: zone.kind === "foreman",
      });

      if (zone.kind === "task" || zone.kind === "blocked") {
        // 工作/受阻都对齐到自己的工位，而不是整个区域中心。
        buildTargetMap.set(agentId, charPos);
      }
    });
  }

  return {
    zones,
    layout,
    buildTargetMap,
    bedInstances: offlineBeds,
  };
}
