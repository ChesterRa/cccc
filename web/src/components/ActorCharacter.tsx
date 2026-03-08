import React, { useMemo, useEffect, useRef } from "react";
import * as THREE from "three";
import { useFrame } from "@react-three/fiber";
import type { AgentState } from "../types";
import { deriveAnimState, deriveStatusLabel, hashCode } from "../utils/actorUtils";

// Shared geometry instances (module-level singletons, never disposed)
const TORSO_GEO = new THREE.BoxGeometry(0.35, 0.5, 0.25);
const HEAD_GEO = new THREE.BoxGeometry(0.28, 0.28, 0.25);
const ARM_GEO = new THREE.BoxGeometry(0.12, 0.4, 0.15);
ARM_GEO.translate(0, -0.2, 0); // pivot at shoulder (top of arm)
const LEG_GEO = new THREE.BoxGeometry(0.14, 0.25, 0.18);
LEG_GEO.translate(0, -0.125, 0); // pivot at hip (top of leg)
// Status ring at character's feet
const RING_GEO = new THREE.RingGeometry(0.35, 0.45, 32);
RING_GEO.rotateX(-Math.PI / 2); // lay flat on ground
const RING_MAT_ACTIVE = new THREE.MeshBasicMaterial({ color: "#4ade80", transparent: true, opacity: 0.5, side: THREE.DoubleSide, depthWrite: false });
const RING_MAT_IDLE = new THREE.MeshBasicMaterial({ color: "#94a3b8", transparent: true, opacity: 0.2, side: THREE.DoubleSide, depthWrite: false });

// MC-style blocky crown: band + 3 tooth points
const CROWN_BAND_GEO = new THREE.BoxGeometry(0.32, 0.05, 0.29);
const CROWN_POINT_GEO = new THREE.BoxGeometry(0.07, 0.09, 0.07);
const CROWN_MAT = new THREE.MeshStandardMaterial({ color: "#fbbf24" });

// Runtime → body color mapping
const RUNTIME_BODY_COLORS: Record<string, string> = {
  claude:   "#38bdf8", // sky blue
  gemini:   "#34d399", // emerald
  codex:    "#22d3ee", // cyan
  grok:     "#f87171", // red
  copilot:  "#94a3b8", // slate
  aider:    "#a78bfa", // violet
  roo:      "#e879f9", // fuchsia
  neovate:  "#d946ef", // fuchsia-deep
  opencode: "#06b6d4", // cyan-deep
  custom:   "#fb923c", // orange
};

// Runtime → face label (MVP text logos)
const RUNTIME_FACE: Record<string, string> = {
  claude:   "C",
  gemini:   "G",
  codex:    "Cx",
  grok:     "Gk",
  copilot:  "Cp",
  aider:    "A",
  roo:      "R",
  neovate:  "N",
  opencode: "O",
  custom:   "?",
};

// Runtime → PNG logo paths (served from /public/logos/, prefixed with Vite base)
const _base = import.meta.env.BASE_URL;
const RUNTIME_LOGO: Record<string, string> = {
  claude: `${_base}logos/claude.png`,
  codex: `${_base}logos/codex.png`,
  gemini: `${_base}logos/gemini.png`,
};

// Cached textures for head face logos
const FACE_TEX_CACHE = new Map<string, THREE.CanvasTexture>();

function getFaceTexture(label: string, bg: string): THREE.CanvasTexture {
  const key = `txt:${label}:${bg}`;
  let t = FACE_TEX_CACHE.get(key);
  if (t) return t;
  const s = 64;
  const c = document.createElement("canvas");
  c.width = s;
  c.height = s;
  const ctx = c.getContext("2d")!;
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, s, s);
  ctx.fillStyle = "#fff";
  ctx.font = label.length > 1 ? "bold 26px monospace" : "bold 36px monospace";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(label, s / 2, s / 2);
  t = new THREE.CanvasTexture(c);
  FACE_TEX_CACHE.set(key, t);
  return t;
}

// PNG texture loader with body-color background fill (covers rounded corners)
function getPngTexture(path: string, bgColor: string): THREE.CanvasTexture {
  const key = `png:${path}:${bgColor}`;
  let t = FACE_TEX_CACHE.get(key);
  if (t) return t;
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  t = new THREE.CanvasTexture(canvas);
  t.colorSpace = THREE.SRGBColorSpace;
  FACE_TEX_CACHE.set(key, t);
  const tex = t;
  const img = new Image();
  img.onload = () => {
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, size, size);
    ctx.drawImage(img, 0, 0, size, size);
    tex.needsUpdate = true;
  };
  img.src = path;
  return t;
}

// Deterministic fallback palette
const PALETTE = [
  "#38bdf8", "#818cf8", "#a78bfa", "#e879f9", "#22d3ee",
  "#2dd4bf", "#34d399", "#fbbf24", "#f87171", "#fb923c",
];

// Re-exports removed: AgentAnimState, deriveAnimState, deriveStatusLabel, PART_INDEX, hashCode
// are now in ../utils/actorUtils.ts (react-refresh requires component-only exports)

/** Format idle seconds to a human-readable duration string */
function formatIdleDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${Math.round(seconds / 3600)}h`;
}

/** Darken a hex color by a factor (0–1). E.g. darkenHex("#4ade80", 0.3) → 30% darker. */
function darkenHex(hex: string, factor: number): string {
  const h = hex.replace("#", "");
  const r = Math.round(parseInt(h.substring(0, 2), 16) * (1 - factor));
  const g = Math.round(parseInt(h.substring(2, 4), 16) * (1 - factor));
  const b = Math.round(parseInt(h.substring(4, 6), 16) * (1 - factor));
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

function agentColor(id: string, runtime?: string): string {
  if (runtime && RUNTIME_BODY_COLORS[runtime]) {
    return RUNTIME_BODY_COLORS[runtime];
  }
  return PALETTE[hashCode(id) % PALETTE.length];
}

// Module-level body material cache: shared across agents with same color/state
const BODY_MAT_CACHE = new Map<string, THREE.MeshStandardMaterial>();
const OFFLINE_MAT = new THREE.MeshStandardMaterial({
  color: "#6b7280", flatShading: true, transparent: true, opacity: 0.55,
});

function getBodyMaterial(color: string, offline: boolean): THREE.MeshStandardMaterial {
  if (offline) return OFFLINE_MAT;
  let mat = BODY_MAT_CACHE.get(color);
  if (!mat) {
    mat = new THREE.MeshStandardMaterial({ color, flatShading: true });
    BODY_MAT_CACHE.set(color, mat);
  }
  return mat;
}

export interface ActorCharacterProps {
  agent: AgentState;
  position: [number, number, number];
  rotationY?: number;
  isDark: boolean;
  role?: string;
  runtime?: string;
  title?: string;
  isRunning?: boolean;
  idleSeconds?: number | null;
  activeTaskName?: string;
  taskStatus?: string;
  focus?: string;
  blockerText?: string;
  onCharacterClick?: () => void;
}

export const ActorCharacter = React.forwardRef<THREE.Group, ActorCharacterProps>(
  function ActorCharacter({ agent, position, rotationY = 0, isDark, role, runtime, title, isRunning, idleSeconds, activeTaskName, taskStatus, focus, blockerText, onCharacterClick }, ref) {
    const color = agentColor(agent.id, runtime);
    const isForeman = role === "foreman";
    const isOffline = isRunning === false;

    // Cache rotation tuple to prevent R3F re-applying on re-render
    // (which would fight useFrame-driven rotation, e.g. sleeping face-up)
    const rotationTuple = useMemo(
      () => [0, rotationY, 0] as [number, number, number],
      [rotationY],
    );

    // Shared cached material (body color; gray + semi-transparent when offline)
    const mat = getBodyMaterial(color, isOffline);

    // Head face texture: PNG logo if available, else text fallback
    const faceLabel = runtime
      ? (RUNTIME_FACE[runtime] || runtime.charAt(0).toUpperCase())
      : agent.id.charAt(0).toUpperCase();
    const logoPath = runtime ? RUNTIME_LOGO[runtime] : undefined;
    const faceMat = useMemo(() => {
      if (isOffline) return OFFLINE_MAT;
      if (logoPath) {
        const tex = getPngTexture(logoPath, color);
        return new THREE.MeshStandardMaterial({ map: tex, flatShading: true });
      }
      const tex = getFaceTexture(faceLabel, color);
      return new THREE.MeshStandardMaterial({ map: tex, flatShading: true });
    }, [logoPath, faceLabel, color, isOffline]);
    useEffect(() => () => { if (faceMat !== OFFLINE_MAT) faceMat.dispose(); }, [faceMat]);
    // Material array: all sides = body color, front face (-Z, index 5) = logo
    const headMats = useMemo(
      () => [mat, mat, mat, mat, mat, faceMat],
      [mat, faceMat],
    );

    // Convert PTY idle_seconds to lastActivityAt (epoch ms) for deriveAnimState
    const lastActivityAt = idleSeconds != null ? Date.now() - idleSeconds * 1000 : undefined;
    const animState = deriveAnimState(agent, isRunning, lastActivityAt, taskStatus);
    const statusLabel = deriveStatusLabel(animState, !!agent.hot?.active_task_id, isForeman);

    // Pre-truncate long text before bubble key to avoid unnecessary texture rebuilds
    const MAX_TASK = 35;
    const MAX_FOCUS = 42;
    const MAX_BLOCKER = 42;
    const _taskText = activeTaskName ? (activeTaskName.length > MAX_TASK ? activeTaskName.slice(0, MAX_TASK) + "\u2026" : activeTaskName) : "";
    const _focusText = focus ? (focus.length > MAX_FOCUS ? focus.slice(0, MAX_FOCUS) + "\u2026" : focus) : "";
    const _blockerText = blockerText ? (blockerText.length > MAX_BLOCKER ? blockerText.slice(0, MAX_BLOCKER) + "\u2026" : blockerText) : "";

    // Idle duration text for bubble (quantized to avoid texture churn)
    const isActive = idleSeconds != null && idleSeconds < 15;
    const _idleText = (idleSeconds != null && isRunning !== false)
      ? formatIdleDuration(idleSeconds)
      : "";

    // Ring state: active (idle<15s), idle (running but idle≥15s), offline (not running)
    // Debounce via ref to prevent flicker
    const ringStateRef = useRef<"active" | "idle" | "off">("off");
    const ringDebounceRef = useRef<number>(0);
    const ringMeshRef = useRef<THREE.Mesh>(null);

    const targetRingState = isOffline ? "off" : isActive ? "active" : "idle";
    if (targetRingState !== ringStateRef.current) {
      // Debounce only active→idle/off direction (prevent flicker on brief output gaps)
      // Transitions toward active apply immediately for responsiveness
      const isDowngrade = ringStateRef.current === "active" && targetRingState !== "active";
      if (isDowngrade) {
        const now = Date.now();
        if (now - ringDebounceRef.current > 3000) {
          ringStateRef.current = targetRingState;
          ringDebounceRef.current = now;
        }
      } else {
        ringStateRef.current = targetRingState;
        ringDebounceRef.current = Date.now();
      }
    }
    const ringState = ringStateRef.current;

    // Per-instance ring material for breathing animation (avoids shared state mutation)
    const ringMatRef = useRef<THREE.MeshBasicMaterial | null>(null);
    if (!ringMatRef.current) {
      ringMatRef.current = RING_MAT_ACTIVE.clone();
    }
    useEffect(() => () => { ringMatRef.current?.dispose(); }, []);
    useEffect(() => () => { document.body.style.cursor = "auto"; }, []);

    // Breathing animation for active ring
    useFrame(({ clock }) => {
      const mesh = ringMeshRef.current;
      const ringMat = ringMatRef.current;
      if (!mesh || !ringMat) return;
      if (ringState === "active") {
        mesh.visible = true;
        ringMat.color.set("#4ade80");
        mesh.material = ringMat;
        // Breathing: opacity oscillates 0.3–0.8
        const t = clock.getElapsedTime() + hashCode(agent.id) * 0.3;
        ringMat.opacity = 0.3 + 0.25 * (1 + Math.sin(t * 2));
      } else if (ringState === "idle") {
        mesh.visible = true;
        mesh.material = RING_MAT_IDLE;
      } else {
        mesh.visible = false;
      }
    });

    // Status bubble texture (GPU sprite replaces Html DOM overlay for performance)
    const _bubbleKey = `${title || agent.id}|${statusLabel.text}|${statusLabel.color}|${_taskText}|${_focusText}|${_blockerText}|${_idleText}|${isActive ? 1 : 0}|${isDark ? 1 : 0}|${color}`;
    const { bubbleTex, bubbleScale } = useMemo(() => {
      const DPR = 2;
      const CW = 220 * DPR;
      const padX = 10 * DPR;
      const padY = 7 * DPR;
      const titleFs = 13 * DPR;
      const statusFs = 11 * DPR;
      const taskFs = 9 * DPR;
      const gap = 4 * DPR;

      const shadowMargin = 10 * DPR; // extra canvas space for drop shadow
      let h = padY + titleFs * 1.3 + gap + statusFs * 1.3;
      if (_idleText) h += gap + taskFs * 1.3;
      if (_taskText) h += gap + taskFs * 1.3;
      if (_focusText) h += gap + taskFs * 1.3;
      if (_blockerText) h += gap + taskFs * 1.3;
      h += padY;
      const boxH = Math.ceil(h);
      const CH = boxH + shadowMargin;

      const canvas = document.createElement("canvas");
      canvas.width = CW + shadowMargin;
      canvas.height = CH;
      const ctx = canvas.getContext("2d")!;

      const ox = shadowMargin / 2; // offset to center box within padded canvas
      const bg = isDark ? "rgba(15,23,42,0.96)" : "rgba(255,255,255,0.98)";
      const border = isDark ? "rgba(100,116,139,0.6)" : "rgba(156,163,175,0.9)";
      ctx.beginPath();
      ctx.roundRect(ox, 0, CW, boxH, 8 * DPR);
      // Drop shadow for depth against green scene
      ctx.shadowColor = "rgba(0,0,0,0.30)";
      ctx.shadowBlur = 6 * DPR;
      ctx.shadowOffsetY = 2 * DPR;
      ctx.fillStyle = bg;
      ctx.fill();
      ctx.shadowColor = "transparent";
      ctx.strokeStyle = border;
      ctx.lineWidth = 1.5 * DPR;
      ctx.stroke();

      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      const cx = ox + CW / 2; // text center x within padded canvas
      let y = padY;
      const maxTW = CW - padX * 2;

      // Title — darken agent color in light mode for contrast on white card
      ctx.font = `700 ${titleFs}px sans-serif`;
      ctx.fillStyle = isDark ? color : darkenHex(color, 0.35);
      let tText = title || agent.id;
      if (ctx.measureText(tText).width > maxTW) {
        while (ctx.measureText(tText + "\u2026").width > maxTW && tText.length > 1) tText = tText.slice(0, -1);
        tText += "\u2026";
      }
      ctx.fillText(tText, cx, y);
      y += titleFs * 1.3 + gap;

      // Status — with active dot prefix for running state
      ctx.font = `600 ${statusFs}px sans-serif`;
      const statusColor = isDark ? statusLabel.color : darkenHex(statusLabel.color, 0.3);
      if (isActive) {
        // Green dot "● 运行中" style
        const dotRadius = 3 * DPR;
        const statusText = `${statusLabel.text}`;
        const textW = ctx.measureText(statusText).width;
        const dotGap = 4 * DPR;
        const totalW = dotRadius * 2 + dotGap + textW;
        const startX = cx - totalW / 2;
        // Draw green dot
        ctx.fillStyle = "#4ade80";
        ctx.beginPath();
        ctx.arc(startX + dotRadius, y + statusFs * 0.55, dotRadius, 0, Math.PI * 2);
        ctx.fill();
        // Draw status text
        ctx.textAlign = "left";
        ctx.fillStyle = statusColor;
        ctx.fillText(statusText, startX + dotRadius * 2 + dotGap, y);
        ctx.textAlign = "center"; // reset
      } else {
        ctx.fillStyle = statusColor;
        ctx.fillText(statusLabel.text, cx, y);
      }
      y += statusFs * 1.3 + gap;

      // Idle duration (small text line)
      if (_idleText) {
        ctx.font = `400 ${taskFs}px sans-serif`;
        ctx.fillStyle = isDark ? "#94a3b8" : "#9ca3af";
        ctx.fillText(`idle ${_idleText}`, cx, y);
        y += taskFs * 1.3 + gap;
      }

      // Task name (pre-truncated via _taskText)
      if (_taskText) {
        ctx.font = `500 ${taskFs}px sans-serif`;
        ctx.fillStyle = isDark ? "#cbd5e1" : "#4b5563";
        let tkText = _taskText;
        if (ctx.measureText(tkText).width > maxTW) {
          while (ctx.measureText(tkText + "\u2026").width > maxTW && tkText.length > 1) tkText = tkText.slice(0, -1);
          tkText += "\u2026";
        }
        ctx.fillText(tkText, cx, y);
        y += taskFs * 1.3 + gap;
      }

      // Focus text (pre-truncated via _focusText)
      if (_focusText) {
        ctx.font = `500 ${taskFs}px sans-serif`;
        ctx.fillStyle = isDark ? "#cbd5e1" : "#4b5563";
        let fText = _focusText;
        if (ctx.measureText(fText).width > maxTW) {
          while (ctx.measureText(fText + "\u2026").width > maxTW && fText.length > 1) fText = fText.slice(0, -1);
          fText += "\u2026";
        }
        ctx.fillText(fText, cx, y);
        y += taskFs * 1.3 + gap;
      }

      // Blocker text (red, pre-truncated via _blockerText)
      if (_blockerText) {
        ctx.font = `500 ${taskFs}px sans-serif`;
        ctx.fillStyle = isDark ? "#f87171" : "#dc2626";
        let bText = _blockerText;
        if (ctx.measureText(bText).width > maxTW) {
          while (ctx.measureText(bText + "\u2026").width > maxTW && bText.length > 1) bText = bText.slice(0, -1);
          bText += "\u2026";
        }
        ctx.fillText(bText, cx, y);
      }

      const tex = new THREE.CanvasTexture(canvas);
      tex.colorSpace = THREE.SRGBColorSpace;
      const actualCW = CW + shadowMargin;
      const worldW = 1.15;
      return { bubbleTex: tex, bubbleScale: [worldW, worldW * (CH / actualCW)] as [number, number] };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [_bubbleKey]);
    useEffect(() => () => { bubbleTex.dispose(); }, [bubbleTex]);

    return (
      <group
        ref={ref}
        position={position}
        rotation={rotationTuple}
        onClick={(e) => { e.stopPropagation(); onCharacterClick?.(); }}
        onPointerOver={() => { document.body.style.cursor = "pointer"; }}
        onPointerOut={() => { document.body.style.cursor = "auto"; }}
      >
        {/* Body parts — named for animation targeting via PART_INDEX (indices 0-5 must stay stable) */}
        <mesh name="torso" position={[0, 0.55, 0]} castShadow geometry={TORSO_GEO} material={mat} />
        <mesh name="head" position={[0, 1.0, 0]} castShadow geometry={HEAD_GEO} material={headMats} />
        <mesh name="leftArm" position={[-0.25, 0.75, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="rightArm" position={[0.25, 0.75, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="leftLeg" position={[-0.1, 0.30, 0]} castShadow geometry={LEG_GEO} material={mat} />
        <mesh name="rightLeg" position={[0.1, 0.30, 0]} castShadow geometry={LEG_GEO} material={mat} />

        {/* Status ring at character's feet (after body parts to preserve PART_INDEX) */}
        <mesh ref={ringMeshRef} position={[0, 0.02, 0]} geometry={RING_GEO} material={RING_MAT_IDLE} visible={false} />

        {/* Foreman crown (MC-style blocky gold crown) — named for animation lookup */}
        {isForeman && (
          <group name="crown" position={[0, 1.165, 0]}>
            <mesh castShadow geometry={CROWN_BAND_GEO} material={CROWN_MAT} />
            <mesh position={[0, 0.065, -0.1]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
            <mesh position={[-0.11, 0.065, 0.09]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
            <mesh position={[0.11, 0.065, 0.09]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
          </group>
        )}

        {/* Status bubble above head — GPU-rendered sprite (no DOM overhead) */}
        <sprite position={[0, isForeman ? 1.75 : 1.65, 0]} scale={[bubbleScale[0], bubbleScale[1], 1]}>
          <spriteMaterial map={bubbleTex} transparent depthWrite={false} />
        </sprite>
      </group>
    );
  },
);
