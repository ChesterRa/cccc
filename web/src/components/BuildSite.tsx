// BuildSite: single building = blueprint + progress → rendered blocks
// Manages solid/ghost split, pop-in animation, and label

import { forwardRef, useMemo, useRef, useEffect, useImperativeHandle } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { InstancedBlocks, type BlockInstance, type InstancedBlocksHandle } from "./InstancedBlocks";
import type { Blueprint } from "../data/blueprints";

export type BuildStatus = "ghost" | "building" | "complete";

export interface BuildSiteHandle {
  /** World position of the next block to be placed (for Agent walking target) */
  getNextBlockWorldPos(): [number, number, number] | null;
  /** Externally trigger placement of next block (future Agent integration, no-op for now) */
  placeNextBlock(): void;
}

export interface BuildSiteProps {
  blueprint: Blueprint;
  progress: number; // 0.0 ~ 1.0
  status: BuildStatus;
  position: [number, number, number];
  scale?: number;
  label?: string;
  isDark: boolean;
}

const GHOST_OPACITY = 0.15;
const POP_DURATION = 0.3; // seconds

// easeOutBack: overshoots then settles (pop feel)
function easeOutBack(x: number): number {
  const c1 = 1.70158;
  const c3 = c1 + 1;
  return 1 + c3 * Math.pow(x - 1, 3) + c1 * Math.pow(x - 1, 2);
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

export const BuildSite = forwardRef<BuildSiteHandle, BuildSiteProps>(
  function BuildSite({ blueprint, progress, status, position, scale = 1, label, isDark }, ref) {
    const bs = blueprint.blockScale * scale;
    const solidRef = useRef<InstancedBlocksHandle>(null);
    const prevCountRef = useRef(0);
    const animMap = useRef(new Map<number, number>()); // block index → spawn start time

    // Sorted blocks by order
    const sortedBlocks = useMemo(
      () => [...blueprint.blocks].sort((a, b) => a.order - b.order),
      [blueprint],
    );

    // Number of solid blocks based on status + progress
    const solidCount =
      status === "ghost" ? 0
      : status === "complete" ? sortedBlocks.length
      : Math.floor(sortedBlocks.length * clamp01(progress));

    // Detect newly placed blocks → start pop animation (building status only)
    useEffect(() => {
      if (status === "building" && solidCount > prevCountRef.current) {
        const now = performance.now() / 1000;
        for (let i = prevCountRef.current; i < solidCount; i++) {
          animMap.current.set(i, now);
        }
      }
      prevCountRef.current = solidCount;
    }, [solidCount, status]);

    // Center blueprint horizontally (x, z); y=0 at group origin
    const cx = -(blueprint.gridSize[0] - 1) * bs / 2;
    const cz = -(blueprint.gridSize[2] - 1) * bs / 2;

    // Solid block instances (useFrame handles pop-in animation scale)
    const solidBlocks = useMemo((): BlockInstance[] =>
      sortedBlocks.slice(0, solidCount).map((bl) => ({
        position: [cx + bl.x * bs, bl.y * bs, cz + bl.z * bs] as [number, number, number],
        color: bl.color,
        scale: bs,
      })),
      [sortedBlocks, solidCount, bs, cx, cz],
    );

    // Ghost block instances (remaining un-built blocks)
    const ghostBlocks = useMemo((): BlockInstance[] => {
      if (status === "complete") return [];
      const start = status === "ghost" ? 0 : solidCount;
      return sortedBlocks.slice(start).map((bl) => ({
        position: [cx + bl.x * bs, bl.y * bs, cz + bl.z * bs] as [number, number, number],
        color: bl.color,
        scale: bs,
      }));
    }, [sortedBlocks, solidCount, status, bs, cx, cz]);

    // Keep solidBlocks ref in sync for useFrame access (avoids stale closure)
    const solidBlocksRef = useRef(solidBlocks);
    useEffect(() => { solidBlocksRef.current = solidBlocks; });

    // Pop animation: update animating blocks each frame via imperative handle
    useFrame(() => {
      if (animMap.current.size === 0) return;
      const handle = solidRef.current;
      if (!handle) return;
      const blocks = solidBlocksRef.current;
      const now = performance.now() / 1000;

      for (const [idx, startTime] of animMap.current) {
        if (idx >= blocks.length) { animMap.current.delete(idx); continue; }
        const bl = blocks[idx];
        const elapsed = now - startTime;
        if (elapsed >= POP_DURATION) {
          handle.setBlock(idx, bl.position, bl.color, bs);
          animMap.current.delete(idx);
        } else {
          handle.setBlock(idx, bl.position, bl.color, bs * easeOutBack(elapsed / POP_DURATION));
        }
      }
      handle.flush();
    });

    // Expose API for future Agent walking integration
    useImperativeHandle(ref, () => ({
      getNextBlockWorldPos() {
        if (solidCount >= sortedBlocks.length) return null;
        const bl = sortedBlocks[solidCount];
        return [
          position[0] + cx + bl.x * bs,
          position[1] + bl.y * bs,
          position[2] + cz + bl.z * bs,
        ];
      },
      placeNextBlock() {
        // Future: externally triggered block placement (no-op, progress-driven for now)
      },
    }), [solidCount, sortedBlocks, position, cx, cz, bs]);

    // Label above tallest block — GPU-rendered sprite (no DOM overhead)
    const labelY = blueprint.gridSize[1] * bs + bs;
    const _labelKey = `${label || ""}|${isDark ? 1 : 0}`;
    const labelState = useMemo(() => {
      if (!label) return null;
      const DPR = 2;
      const padX = 6 * DPR;
      const padY = 3 * DPR;
      const fs = 10 * DPR;

      // Measure text width to fit canvas
      const measureCanvas = document.createElement("canvas");
      const mCtx = measureCanvas.getContext("2d")!;
      mCtx.font = `600 ${fs}px sans-serif`;
      let displayLabel = label;
      const maxTextW = 180 * DPR;
      if (mCtx.measureText(displayLabel).width > maxTextW) {
        while (mCtx.measureText(displayLabel + "\u2026").width > maxTextW && displayLabel.length > 1) displayLabel = displayLabel.slice(0, -1);
        displayLabel += "\u2026";
      }
      const textW = mCtx.measureText(displayLabel).width;
      const CW = Math.ceil(textW + padX * 2);
      const CH = Math.ceil(fs * 1.3 + padY * 2);

      const canvas = document.createElement("canvas");
      canvas.width = CW;
      canvas.height = CH;
      const ctx = canvas.getContext("2d")!;

      ctx.beginPath();
      ctx.roundRect(0, 0, CW, CH, 6 * DPR);
      ctx.fillStyle = isDark ? "rgba(15,23,42,0.85)" : "rgba(255,255,255,0.9)";
      ctx.fill();

      ctx.font = `600 ${fs}px sans-serif`;
      ctx.fillStyle = isDark ? "#e2e8f0" : "#1e293b";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(displayLabel, CW / 2, CH / 2);

      const tex = new THREE.CanvasTexture(canvas);
      tex.colorSpace = THREE.SRGBColorSpace;
      const worldW = 0.7;
      return { tex, scale: [worldW, worldW * (CH / CW)] as [number, number], label: displayLabel };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [_labelKey]);
    useEffect(() => { return () => { labelState?.tex.dispose(); }; }, [labelState]);

    return (
      <group position={position}>
        {solidBlocks.length > 0 && (
          <InstancedBlocks ref={solidRef} blocks={solidBlocks} />
        )}
        {ghostBlocks.length > 0 && (
          <InstancedBlocks blocks={ghostBlocks} opacity={GHOST_OPACITY} transparent />
        )}
        {labelState && (
          <sprite position={[0, labelY, 0]} scale={[labelState.scale[0], labelState.scale[1], 1]}>
            <spriteMaterial map={labelState.tex} transparent depthWrite={false} />
          </sprite>
        )}
      </group>
    );
  },
);
