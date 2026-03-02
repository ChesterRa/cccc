import React, { useMemo, useEffect } from "react";
import { Html } from "@react-three/drei";
import * as THREE from "three";
import type { AgentState } from "../types";
import { deriveAnimState, deriveStatusLabel, hashCode } from "../utils/actorUtils";

// Shared geometry instances (module-level singletons, never disposed)
const TORSO_GEO = new THREE.BoxGeometry(0.35, 0.5, 0.25);
const HEAD_GEO = new THREE.BoxGeometry(0.28, 0.28, 0.25);
const ARM_GEO = new THREE.BoxGeometry(0.12, 0.4, 0.15);
const LEG_GEO = new THREE.BoxGeometry(0.14, 0.25, 0.18);
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
}

export const ActorCharacter = React.forwardRef<THREE.Group, ActorCharacterProps>(
  function ActorCharacter({ agent, position, rotationY = 0, isDark, role, runtime, title, isRunning, activeTaskName }, ref) {
    const color = agentColor(agent.id, runtime);
    const isForeman = role === "foreman";
    const isOffline = isRunning === false;

    // Shared material per agent (body color; gray + semi-transparent when offline)
    const mat = useMemo(() => {
      if (isOffline) {
        return new THREE.MeshStandardMaterial({
          color: "#6b7280",
          flatShading: true,
          transparent: true,
          opacity: 0.55,
        });
      }
      return new THREE.MeshStandardMaterial({ color, flatShading: true });
    }, [color, isOffline]);
    useEffect(() => () => { mat.dispose(); }, [mat]);

    // Head face texture: PNG logo if available, else text fallback
    const faceLabel = runtime
      ? (RUNTIME_FACE[runtime] || runtime.charAt(0).toUpperCase())
      : agent.id.charAt(0).toUpperCase();
    const logoPath = runtime ? RUNTIME_LOGO[runtime] : undefined;
    const faceMat = useMemo(() => {
      if (isOffline) {
        return new THREE.MeshStandardMaterial({
          color: "#6b7280",
          flatShading: true,
          transparent: true,
          opacity: 0.55,
        });
      }
      if (logoPath) {
        const tex = getPngTexture(logoPath, color);
        return new THREE.MeshStandardMaterial({ map: tex, flatShading: true });
      }
      const tex = getFaceTexture(faceLabel, color);
      return new THREE.MeshStandardMaterial({ map: tex, flatShading: true });
    }, [logoPath, faceLabel, color, isOffline]);
    useEffect(() => () => { faceMat.dispose(); }, [faceMat]);
    // Material array: all sides = body color, front face (-Z, index 5) = logo
    const headMats = useMemo(
      () => [mat, mat, mat, mat, mat, faceMat],
      [mat, faceMat],
    );

    const animState = deriveAnimState(agent, isRunning);
    const statusLabel = deriveStatusLabel(animState, !!agent.active_task_id, isForeman);

    return (
      <group ref={ref} position={position} rotation={[0, rotationY, 0]}>
        {/* Body parts — named for animation targeting via PART_INDEX */}
        <mesh name="torso" position={[0, 0.55, 0]} castShadow geometry={TORSO_GEO} material={mat} />
        <mesh name="head" position={[0, 1.0, 0]} castShadow geometry={HEAD_GEO} material={headMats} />
        <mesh name="leftArm" position={[-0.25, 0.5, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="rightArm" position={[0.25, 0.5, 0]} castShadow geometry={ARM_GEO} material={mat} />
        <mesh name="leftLeg" position={[-0.1, 0.12, 0]} castShadow geometry={LEG_GEO} material={mat} />
        <mesh name="rightLeg" position={[0.1, 0.12, 0]} castShadow geometry={LEG_GEO} material={mat} />

        {/* Foreman crown (MC-style blocky gold crown) */}
        {isForeman && (
          <group position={[0, 1.165, 0]}>
            <mesh castShadow geometry={CROWN_BAND_GEO} material={CROWN_MAT} />
            <mesh position={[0, 0.065, -0.1]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
            <mesh position={[-0.11, 0.065, 0.09]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
            <mesh position={[0.11, 0.065, 0.09]} castShadow geometry={CROWN_POINT_GEO} material={CROWN_MAT} />
          </group>
        )}

        {/* Status bubble above head */}
        <Html
          position={[0, isForeman ? 1.65 : 1.55, 0]}
          center
          distanceFactor={6}
          style={{ pointerEvents: "none", userSelect: "none" }}
        >
          <div
            style={{
              background: isDark ? "rgba(15,23,42,0.92)" : "rgba(255,255,255,0.95)",
              border: `1px solid ${isDark ? "rgba(100,116,139,0.4)" : "rgba(209,213,219,0.8)"}`,
              borderRadius: 8,
              padding: "4px 8px",
              minWidth: 48,
              maxWidth: 180,
              textAlign: "center",
              boxShadow: isDark
                ? "0 2px 8px rgba(0,0,0,0.4)"
                : "0 2px 8px rgba(0,0,0,0.1)",
            }}
          >
            {/* Agent title */}
            <div
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: color,
                lineHeight: "14px",
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {title || agent.id}
            </div>

            {/* Short status label */}
            <div
              style={{
                fontSize: 9,
                color: statusLabel.color,
                marginTop: 2,
                fontWeight: 500,
                lineHeight: "12px",
              }}
            >
              {statusLabel.text}
            </div>

            {/* Active task name */}
            {activeTaskName && (
              <div
                style={{
                  fontSize: 8,
                  color: isDark ? "#94a3b8" : "#9ca3af",
                  marginTop: 2,
                  lineHeight: "10px",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {activeTaskName.length > 25
                  ? activeTaskName.slice(0, 25) + "..."
                  : activeTaskName}
              </div>
            )}
          </div>
        </Html>
      </group>
    );
  },
);
