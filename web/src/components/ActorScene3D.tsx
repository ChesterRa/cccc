import { Suspense, useMemo, useRef, useState, useEffect, useCallback } from "react";
import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import { ActorCharacter } from "./ActorCharacter";
import { SemanticMap } from "./SemanticMap";
import { PanoramaRoom } from "./PanoramaRoom";
import { MCGround, InstancedBeds, type BedInstance } from "./MCFurniture";
import { useCharacterAnimation } from "../hooks/useCharacterAnimation";
import { useFirstPersonFollow } from "../hooks/useFirstPersonFollow";
import type { AgentState, Actor, Task, GroupContext } from "../types";
import { buildSemanticMapState, SEMANTIC_MAP_SCENE_EXTENT } from "../utils/panoramaSemanticMap";
import * as THREE from "three";

interface ActorScene3DProps {
  agents: AgentState[];
  actors?: Actor[];
  tasks?: Task[];
  tasksSummary?: GroupContext["tasks_summary"];
  projectStatus?: string | null;
  isDark: boolean;
  groupId?: string;
  className?: string;
}

function agentActiveTaskId(agent: AgentState): string {
  return String(agent.hot?.active_task_id || "").trim();
}

function agentFocus(agent: AgentState): string {
  return String(agent.hot?.focus || "").trim();
}

function agentBlocker(agent: AgentState): string | undefined {
  const blockers = Array.isArray(agent.hot?.blockers) ? agent.hot.blockers : [];
  return blockers.length > 0 ? String(blockers[0] || "").trim() || undefined : undefined;
}

function taskLabel(task: Task): string {
  return String(task.title || task.id || "").trim();
}

// ── Scene ──

interface SceneProps {
  agents: AgentState[];
  actors?: Actor[];
  tasks?: Task[];
  tasksSummary?: GroupContext["tasks_summary"];
  projectStatus?: string | null;
  isDark: boolean;
  groupId?: string;
  camZ: number;
  followTarget: string | null;
  onCharacterClick: (agentId: string) => void;
}

function Scene({ agents, actors, tasks, tasksSummary: _tasksSummary, projectStatus, isDark, groupId: _groupId, camZ, followTarget, onCharacterClick }: SceneProps) {
  const characterRefs = useRef<Map<string, THREE.Group>>(new Map());

  const actorMap = useMemo(() => {
    const m = new Map<string, Actor>();
    for (const a of actors || []) m.set(a.id, a);
    return m;
  }, [actors]);

  const taskMap = useMemo(() => {
    const m = new Map<string, { idx: number; name: string; status?: string | null }>();
    if (!tasks || tasks.length === 0) return m;
    for (let i = 0; i < tasks.length; i++) {
      m.set(tasks[i].id, { idx: i, name: taskLabel(tasks[i]), status: tasks[i].status });
    }
    return m;
  }, [tasks]);

  const semanticMap = useMemo(
    () => buildSemanticMapState(agents, actorMap, tasks),
    [agents, actorMap, tasks],
  );
  const layout = semanticMap.layout;
  const buildTargetMap = semanticMap.buildTargetMap;

  const taskStatusMap = useMemo(() => {
    const m = new Map<string, string>();
    if (!tasks) return m;
    for (const t of tasks) {
      if (t.status) m.set(t.id, t.status);
    }
    return m;
  }, [tasks]);

  useCharacterAnimation({
    agents, actorMap, layout, buildTargetMap, characterRefs, taskStatusMap,
    staticMode: true,
  });

  useFirstPersonFollow({ targetId: followTarget, characterRefs });

  const bedInstances: BedInstance[] = semanticMap.bedInstances;

  return (
    <>
      <ambientLight intensity={isDark ? 0.42 : 0.72} />
      <directionalLight
        position={[4.5, 7.5, 4]}
        intensity={isDark ? 0.85 : 1}
        castShadow
        shadow-mapSize-width={1024}
        shadow-mapSize-height={1024}
      />
      <directionalLight
        position={[-4, 4.5, -2.5]}
        intensity={isDark ? 0.2 : 0.3}
      />
      <pointLight
        position={[0, 4.25, -2.8]}
        intensity={isDark ? 1 : 0.8}
        distance={18}
        decay={2}
        color={isDark ? "#67e8f9" : "#60a5fa"}
      />

      <MCGround />

      <PanoramaRoom zones={semanticMap.zones} />

      <SemanticMap zones={semanticMap.zones} isDark={isDark} projectStatus={projectStatus} />

      <InstancedBeds beds={bedInstances} />

      {agents.map((agent) => {
        const actor = actorMap.get(agent.id);
        const running = actor?.running !== false && actor?.enabled !== false;
        const item = layout.get(agent.id);
        const activeTaskId = agentActiveTaskId(agent);
        const taskEntry = activeTaskId ? taskMap.get(activeTaskId) : undefined;
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
            idleSeconds={actor?.idle_seconds}
            activeTaskName={taskEntry?.name.replace(/^T\d+:\s*/, "")}
            taskStatus={taskEntry?.status || undefined}
            focus={agentFocus(agent) || undefined}
            blockerText={agentBlocker(agent)}
            onCharacterClick={() => onCharacterClick(agent.id)}
          />
        );
      })}

      <OrbitControls
        makeDefault
        enableDamping={false}
        enabled={followTarget === null}
        enablePan={true}
        enableZoom={true}
        enableRotate={true}
        minDistance={2}
        maxDistance={camZ * 3}
        maxPolarAngle={Math.PI / 2.05}
        target={[0, 0.5, 0]}
        mouseButtons={{ LEFT: THREE.MOUSE.PAN, MIDDLE: THREE.MOUSE.DOLLY, RIGHT: THREE.MOUSE.ROTATE }}
        screenSpacePanning={true}
      />
    </>
  );
}

// ── WebGPU / WebGL renderer selection ──

type RenderMode = "loading" | "webgpu" | "webgl";

export function ActorScene3D({ agents, actors, tasks, tasksSummary, projectStatus, isDark, groupId, className }: ActorScene3DProps) {
  const [followTarget, setFollowTarget] = useState<string | null>(null);

  const handleCharacterClick = useCallback((agentId: string) => {
    setFollowTarget((prev) => (prev === agentId ? null : agentId));
  }, []);

  const camZ = useMemo(() => {
    return Math.max(5, SEMANTIC_MAP_SCENE_EXTENT * 2 + 1);
  }, []);

  // Phase 1: detect WebGPU + dynamically load three/webgpu module
  const [renderMode, setRenderMode] = useState<RenderMode>("loading");
  const gpuModRef = useRef<any>(null);

  useEffect(() => {
    // WebGPU is opt-in via ?webgpu URL param (R3F v8 has limited WebGPU compat)
    const wantGPU =
      typeof window !== "undefined" &&
      new URLSearchParams(window.location.search).has("webgpu");

    if (!wantGPU || !navigator.gpu) {
      setRenderMode("webgl");
      return;
    }

    let cancelled = false;
    import("three/webgpu")
      .then((mod) => {
        if (cancelled) return;
        gpuModRef.current = mod;
        setRenderMode("webgpu");
      })
      .catch(() => {
        if (cancelled) return;
        setRenderMode("webgl");
      });
    return () => { cancelled = true; };
  }, []);

  // Stable gl factory for WebGPU -- avoids re-creation on every render.
  // Uses a render-guard so R3F can keep frameloop="always" (required for
  // OrbitControls) while WebGPURenderer.init() resolves asynchronously.
  const createWebGPURenderer = useCallback((canvas: HTMLCanvasElement) => {
    const GPU = gpuModRef.current;
    const renderer = new GPU.WebGPURenderer({
      canvas,
      antialias: true,
      alpha: false,
    });
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;

    // XR stub -- prevents R3F v8 "xr.addEventListener is not a function" error
    if (!(renderer as any).xr) {
      (renderer as any).xr = {
        addEventListener: () => {},
        removeEventListener: () => {},
        getSession: () => null,
        setSession: () => Promise.resolve(),
        enabled: false,
        isPresenting: false,
        setReferenceSpaceType: () => {},
        getReferenceSpace: () => null,
        getCamera: () => new THREE.PerspectiveCamera(),
        setAnimationLoop: () => {},
        dispose: () => {},
      };
    }

    // Guard: R3F calls gl.render() every frame via its rAF loop.
    // WebGPURenderer.render() throws if called before init() resolves,
    // so we patch it with a no-op until the backend is ready.
    const nativeRender = renderer.render.bind(renderer);
    let ready = false;
    renderer.render = (scene: THREE.Scene, camera: THREE.Camera) => {
      if (ready) nativeRender(scene, camera);
    };

    renderer.init()
      .then(() => {
        console.warn("[ActorScene3D] WebGPU renderer initialized");
        ready = true;
      })
      .catch((err: unknown) => {
        console.warn("[ActorScene3D] WebGPU init failed, reloading with WebGL:", err);
        setRenderMode("webgl");
      });

    return renderer;
  }, []);

  // Placeholder while detecting WebGPU
  if (renderMode === "loading") {
    return (
      <div
        className={className}
        style={{
          minHeight: 280,
          borderRadius: 12,
          background: isDark ? "#0f172a" : "#dbe4ee",
        }}
      />
    );
  }

  const cameraConfig = {
    position: [camZ * 0.55, camZ * 0.42, camZ * 0.88] as [number, number, number],
    fov: 42,
    near: 0.1,
    far: 100,
  };

  const canvasStyle = {
    borderRadius: 12,
    background: isDark ? "#0f172a" : "#dbe4ee",
  };

  const glProp = renderMode === "webgpu"
    ? (createWebGPURenderer as any)
    : { antialias: true, alpha: false };

  return (
    <div className={className} style={{ minHeight: 280 }}>
      <Canvas
        key={renderMode}
        frameloop="always"
        shadows={{ type: THREE.PCFShadowMap }}
        camera={cameraConfig}
        style={canvasStyle}
        gl={glProp}
        onPointerMissed={() => setFollowTarget(null)}
      >
        <Suspense fallback={null}>
          <Scene agents={agents} actors={actors} tasks={tasks} tasksSummary={tasksSummary} projectStatus={projectStatus} isDark={isDark} groupId={groupId} camZ={camZ} followTarget={followTarget} onCharacterClick={handleCharacterClick} />
        </Suspense>
      </Canvas>
    </div>
  );
}
