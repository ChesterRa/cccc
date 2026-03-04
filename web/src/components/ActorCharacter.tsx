import React, { useMemo, useEffect } from "react";
import * as THREE from "three";
import type { AgentState } from "../types";
import { deriveAnimState, deriveStatusLabel, hashCode } from "../utils/actorUtils";

// Shared geometry instances (module-level singletons, never disposed)
const TORSO_GEO = new THREE.BoxGeometry(0.35, 0.5, 0.25);
const HEAD_GEO = new THREE.BoxGeometry(0.28, 0.28, 0.25);
const ARM_GEO = new THREE.BoxGeometry(0.12, 0.4, 0.15);
ARM_GEO.translate(0, -0.2, 0); // pivot at shoulder (top of arm)
const LEG_GEO = new THREE.BoxGeometry(0.14, 0.25, 0.18);
LEG_GEO.translate(0, -0.125, 0); // pivot at hip (top of leg)
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
  activeTaskName?: string;
  focus?: string;
  blockerText?: string;
}

export const ActorCharacter = React.forwardRef<THREE.Group, ActorCharacterProps>(
  function ActorCharacter({ agent, position, rotationY = 0, isDark, role, runtime, title, isRunning, activeTaskName, focus, blockerText }, ref) {
    const color = agentColor(agent.id, runtime);
    const isForeman = role === "foreman";
    const isOffline = isRunning === false;

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

    const animState = deriveAnimState(agent, isRunning);
    const statusLabel = deriveStatusLabel(animState, !!agent.active_task_id, isForeman);

    // Pre-truncate long text before bubble key to avoid unnecessary texture rebuilds
    const MAX_TASK = 35;
    const MAX_FOCUS = 42;
    const MAX_BLOCKER = 42;
    const _taskText = activeTaskName ? (activeTaskName.length > MAX_TASK ? activeTaskName.slice(0, MAX_TASK) + "\u2026" : activeTaskName) : "";
    const _focusText = focus ? (focus.length > MAX_FOCUS ? focus.slice(0, MAX_FOCUS) + "\u2026" : focus) : "";
    const _blockerText = blockerText ? (blockerText.length > MAX_BLOCKER ? blockerText.slice(0, MAX_BLOCKER) + "\u2026" : blockerText) : "";

    // Status bubble texture (GPU sprite replaces Html DOM overlay for performance)
    const _bubbleKey = `${title || agent.id}|${statusLabel.text}|${statusLabel.color}|${_taskText}|${_focusText}|${_blockerText}|${isDark ? 1 : 0}|${color}`;
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

      // Title
      ctx.font = `700 ${titleFs}px sans-serif`;
      ctx.fillStyle = color;
      let tText = title || agent.id;
      if (ctx.measureText(tText).width > maxTW) {
        while (ctx.measureText(tText + "\u2026").width > maxTW && tText.length > 1) tText = tText.slice(0, -1);
        tText += "\u2026";
      }
      ctx.fillText(tText, cx, y);
      y += titleFs * 1.3 + gap;

      // Status
      ctx.font = `600 ${statusFs}px sans-serif`;
      ctx.fillStyle = statusLabel.color;
      ctx.fillText(statusLabel.text, cx, y);
      y += statusFs * 1.3 + gap;

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
      <group ref={ref} position={position} rotation={[0, rotationY, 0]}>
        {/* Body parts — named for animation targeting via PART_INDEX */}
        <mesh name="torso" position={[0, 0.55, 0]} castShadow geometry={TORSO_GEO} material={mat} />
        <mesh name="head" position={[0, 1.0, 0]} castShadow geometry={HEAD_GEO} material={headMats} />
        <mesh name="leftArm" position={[-0.25, 0.75, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="rightArm" position={[0.25, 0.75, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="leftLeg" position={[-0.1, 0.30, 0]} castShadow geometry={LEG_GEO} material={mat} />
        <mesh name="rightLeg" position={[0.1, 0.30, 0]} castShadow geometry={LEG_GEO} material={mat} />

        {/* Foreman crown (MC-style blocky gold crown) */}
        {isForeman && (
          <group position={[0, 1.165, 0]}>
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
