import { Suspense, useMemo, useRef } from "react";
import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import { ActorCharacter } from "./ActorCharacter";
import { BuildZone } from "./BuildZone";
import { MCGround, MCBed } from "./MCFurniture";
import { computeGridPosition } from "../utils/buildLayout";
import { useCharacterAnimation, type LayoutItem } from "../hooks/useCharacterAnimation";
import type { AgentState, Actor, Task } from "../types";
import * as THREE from "three";

interface ActorScene3DProps {
  agents: AgentState[];
  actors?: Actor[];
  tasks?: Task[];
  isDark: boolean;
  groupId?: string;
  className?: string;
}

// ── Worker layout (construction site mode) ──

function siteRadius(agentCount: number): number {
  return Math.max(2, agentCount * 0.6 + 1);
}

function computeWorkerLayout(
  agents: AgentState[],
  actorMap: Map<string, Actor>,
  buildTargetMap: Map<string, [number, number, number]>,
): Map<string, LayoutItem> {
  const count = agents.length;
  if (count === 0) return new Map();

  const foremanIdx = agents.findIndex((a) => actorMap.get(a.id)?.role === "foreman");
  const radius = siteRadius(count);

  const bedZBase = -radius - 1.2;
  const bedSpacing = 1.2;

  const items = new Map<string, LayoutItem>();
  const idleAgentIds: string[] = [];

  // First pass: assign foreman + task-assigned workers
  // buildTargetMap already includes per-worker spread for co-workers
  for (let i = 0; i < count; i++) {
    const agentId = agents[i].id;
    const bx = (i - (count - 1) / 2) * bedSpacing;
    const bedPosition: [number, number, number] = [bx, 0, bedZBase];
    const bedRotation = 0;
    const buildTarget = buildTargetMap.get(agentId);

    if (i === foremanIdx) {
      items.set(agentId, {
        agentId,
        charPos: [radius * 1.0, 0, -radius * 0.4],
        charRotY: Math.PI * 0.8,
        bedPos: bedPosition,
        bedRotY: bedRotation,
        isForeman: true,
      });
    } else if (buildTarget) {
      const wx = buildTarget[0];
      const wz = buildTarget[2] + 1.0;
      const ry = Math.atan2(buildTarget[0] - wx, buildTarget[2] - wz) + Math.PI;
      items.set(agentId, {
        agentId,
        charPos: [wx, 0, wz],
        charRotY: ry,
        bedPos: bedPosition,
        bedRotY: bedRotation,
        isForeman: false,
      });
    } else {
      // Idle worker: store bed info, defer position
      idleAgentIds.push(agentId);
      items.set(agentId, {
        agentId,
        charPos: [0, 0, 0],
        charRotY: 0,
        bedPos: bedPosition,
        bedRotY: bedRotation,
        isForeman: false,
      });
    }
  }

  // Second pass: spread idle workers on arc behind build zone
  const idleCount = idleAgentIds.length;
  if (idleCount > 0) {
    const idleRadius = radius * 1.3;
    const spread = Math.PI * 0.7;
    const baseAngle = Math.PI; // behind scene (negative z)
    for (let j = 0; j < idleCount; j++) {
      const angle = idleCount > 1
        ? baseAngle + ((j / (idleCount - 1)) - 0.5) * spread
        : baseAngle;
      const x = Math.sin(angle) * idleRadius;
      const z = Math.cos(angle) * idleRadius;
      const ry = Math.atan2(-x, -z) + Math.PI; // face center (mesh faces -Z)
      const item = items.get(idleAgentIds[j])!;
      item.charPos = [x, 0, z];
      item.charRotY = ry;
    }
  }

  return items;
}

// ── Scene ──

interface SceneProps {
  agents: AgentState[];
  actors?: Actor[];
  tasks?: Task[];
  isDark: boolean;
  groupId?: string;
  camZ: number;
}

function Scene({ agents, actors, tasks, isDark, groupId: _groupId, camZ }: SceneProps) {
  const characterRefs = useRef<Map<string, THREE.Group>>(new Map());

  const actorMap = useMemo(() => {
    const m = new Map<string, Actor>();
    for (const a of actors || []) m.set(a.id, a);
    return m;
  }, [actors]);

  const taskMap = useMemo(() => {
    const m = new Map<string, { idx: number; name: string }>();
    if (!tasks || tasks.length === 0) return m;
    for (let i = 0; i < tasks.length; i++) {
      m.set(tasks[i].id, { idx: i, name: tasks[i].name });
    }
    return m;
  }, [tasks]);

  // Filter tasks for BuildZone display: all active + up to 3 most recent done
  const visibleTasks = useMemo(() => {
    if (!tasks || tasks.length === 0) return [];
    const active = tasks.filter(t => t.status !== "done" && t.status !== "archived");
    const done = tasks.filter(t => t.status === "done" || t.status === "archived");
    return [...active, ...done.slice(0, 3)];
  }, [tasks]);

  const buildTargetMap = useMemo(() => {
    const map = new Map<string, [number, number, number]>();
    // Group agents by task to spread co-workers apart
    const taskAgents = new Map<string, string[]>();
    for (const agent of agents) {
      if (!agent.active_task_id) continue;
      const entry = taskMap.get(agent.active_task_id);
      if (!entry) continue;
      const list = taskAgents.get(agent.active_task_id) ?? [];
      list.push(agent.id);
      taskAgents.set(agent.active_task_id, list);
    }
    // Assign per-worker offset so co-workers don't overlap
    for (const [taskId, agentIds] of taskAgents) {
      const entry = taskMap.get(taskId)!;
      const base = computeGridPosition(entry.idx, taskMap.size);
      for (let j = 0; j < agentIds.length; j++) {
        const spread = agentIds.length > 1
          ? (j - (agentIds.length - 1) / 2) * 1.0
          : 0;
        map.set(agentIds[j], [base[0] + spread, base[1], base[2]]);
      }
    }
    // Fallback: task assignee field (for agents without active_task_id)
    if (tasks) {
      const agentIds = new Set(agents.map(a => a.id));
      for (const task of tasks) {
        if (task.assignee && agentIds.has(task.assignee) && !map.has(task.assignee)) {
          const entry = taskMap.get(task.id);
          if (entry) {
            map.set(task.assignee, computeGridPosition(entry.idx, taskMap.size));
          }
        }
      }
    }
    return map;
  }, [agents, tasks, taskMap]);

  const layout = useMemo(
    () => computeWorkerLayout(agents, actorMap, buildTargetMap),
    [agents, actorMap, buildTargetMap],
  );

  useCharacterAnimation({
    agents, actorMap, layout, buildTargetMap, characterRefs,
    staticMode: true,
  });

  return (
    <>
      <ambientLight intensity={isDark ? 0.35 : 0.6} />
      <directionalLight
        position={[5, 8, 5]}
        intensity={isDark ? 0.7 : 0.9}
        castShadow
        shadow-mapSize-width={1024}
        shadow-mapSize-height={1024}
      />
      <directionalLight
        position={[-3, 4, -3]}
        intensity={isDark ? 0.15 : 0.25}
      />

      <MCGround />

      {visibleTasks.length > 0 && (
        <BuildZone tasks={visibleTasks} baseZ={0} isDark={isDark} />
      )}

      {[...layout.values()].map((item) => (
        <MCBed
          key={`bed-${item.agentId}`}
          position={item.bedPos}
          rotationY={item.bedRotY}
        />
      ))}

      {agents.map((agent) => {
        const actor = actorMap.get(agent.id);
        const running = actor?.running !== false && actor?.enabled !== false;
        const item = layout.get(agent.id);
        const taskEntry = agent.active_task_id ? taskMap.get(agent.active_task_id) : undefined;
        return (
          <ActorCharacter
            key={agent.id}
            ref={(el: THREE.Group | null) => {
              if (el) characterRefs.current.set(agent.id, el);
              else characterRefs.current.delete(agent.id);
            }}
            agent={agent}
            position={item?.charPos || [0, 0, 0]}
            rotationY={item?.charRotY}
            isDark={isDark}
            role={actor?.role}
            runtime={actor?.runtime}
            title={actor?.title}
            isRunning={running}
            activeTaskName={taskEntry?.name.replace(/^T\d+:\s*/, "")}
          />
        );
      })}

      <OrbitControls
        enablePan={true}
        enableZoom={true}
        enableRotate={true}
        minDistance={2}
        maxDistance={camZ * 3}
        maxPolarAngle={Math.PI / 2.05}
        target={[0, 0.5, 0]}
      />
    </>
  );
}

export function ActorScene3D({ agents, actors, tasks, isDark, groupId, className }: ActorScene3DProps) {
  const taskCount = tasks?.length ?? 0;
  const camZ = useMemo(() => {
    const radius = siteRadius(agents.length);
    const buildDepth = taskCount > 0 ? Math.ceil(taskCount / 3) * 2.5 + 2 : 0;
    const restAreaDepth = 2.5;
    return Math.max(5, radius + buildDepth + restAreaDepth + 2);
  }, [agents.length, taskCount]);

  return (
    <div className={className} style={{ minHeight: 280 }}>
      <Canvas
        shadows={{ type: THREE.PCFShadowMap }}
        camera={{
          position: [camZ * 0.6, camZ * 0.5, camZ],
          fov: 45,
          near: 0.1,
          far: 100,
        }}
        style={{
          borderRadius: 12,
          background: isDark ? "#191970" : "#87CEEB",
        }}
        gl={{ antialias: true, alpha: false }}
      >
        <Suspense fallback={null}>
          <Scene agents={agents} actors={actors} tasks={tasks} isDark={isDark} groupId={groupId} camZ={camZ} />
        </Suspense>
      </Canvas>
    </div>
  );
}
