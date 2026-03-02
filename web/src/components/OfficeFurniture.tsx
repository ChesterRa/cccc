import React from "react";
import * as THREE from "three";

// ── Minecraft-style color palette ──
const MC = {
  oakWood:    "#BC9862",
  darkOak:    "#4C3320",
  iron:       "#D8D8D8",
  screenOn:   "#55FF55", // green terminal
  screenOff:  "#1A1A1A",
};

// ── Shared geometry singletons (all boxes = Minecraft style) ──
// Desk
const DESK_TOP_GEO = new THREE.BoxGeometry(0.9, 0.06, 0.5);
const DESK_LEG_GEO = new THREE.BoxGeometry(0.06, 0.4, 0.06);
// Monitor
const MONITOR_SCREEN_GEO = new THREE.BoxGeometry(0.5, 0.35, 0.04);
const MONITOR_STAND_GEO = new THREE.BoxGeometry(0.06, 0.15, 0.06);
const MONITOR_BASE_GEO = new THREE.BoxGeometry(0.2, 0.03, 0.12);

// ── Shared materials ──
const OAK_MAT = new THREE.MeshStandardMaterial({ color: MC.oakWood, flatShading: true });
const DARK_OAK_MAT = new THREE.MeshStandardMaterial({ color: MC.darkOak, flatShading: true });
const IRON_MAT = new THREE.MeshStandardMaterial({ color: MC.iron, flatShading: true });
const SCREEN_ON_MAT = new THREE.MeshStandardMaterial({
  color: MC.screenOn,
  emissive: MC.screenOn,
  emissiveIntensity: 0.4,
  flatShading: true,
});
const SCREEN_OFF_MAT = new THREE.MeshStandardMaterial({ color: MC.screenOff, flatShading: true });

// ── Desk component ──
export function Desk({ position }: { position: [number, number, number] }) {
  const [x, y, z] = position;
  const legY = y + 0.2;
  const topY = y + 0.4;
  const hx = 0.38; // half desk width - leg inset
  const hz = 0.18; // half desk depth - leg inset

  return (
    <group position={[x, 0, z]}>
      {/* Desktop */}
      <mesh position={[0, topY, 0]} castShadow receiveShadow geometry={DESK_TOP_GEO} material={OAK_MAT} />
      {/* 4 Legs */}
      <mesh position={[-hx, legY, -hz]} castShadow geometry={DESK_LEG_GEO} material={DARK_OAK_MAT} />
      <mesh position={[hx, legY, -hz]} castShadow geometry={DESK_LEG_GEO} material={DARK_OAK_MAT} />
      <mesh position={[-hx, legY, hz]} castShadow geometry={DESK_LEG_GEO} material={DARK_OAK_MAT} />
      <mesh position={[hx, legY, hz]} castShadow geometry={DESK_LEG_GEO} material={DARK_OAK_MAT} />
    </group>
  );
}

// ── Monitor component ──
export interface MonitorProps {
  position: [number, number, number];
  isOn?: boolean;
}

export function Monitor({ position, isOn = false }: MonitorProps) {
  const [x, y, z] = position;
  const baseY = y;
  const standY = baseY + 0.09;
  const screenY = standY + 0.25;

  return (
    <group position={[x, 0, z]}>
      {/* Base */}
      <mesh position={[0, baseY, 0]} castShadow geometry={MONITOR_BASE_GEO} material={IRON_MAT} />
      {/* Stand */}
      <mesh position={[0, standY, 0]} castShadow geometry={MONITOR_STAND_GEO} material={IRON_MAT} />
      {/* Screen */}
      <mesh
        position={[0, screenY, 0]}
        castShadow
        geometry={MONITOR_SCREEN_GEO}
        material={isOn ? SCREEN_ON_MAT : SCREEN_OFF_MAT}
      />
    </group>
  );
}

// ── Workstation = Desk + Monitor (combined) ──
export interface WorkstationProps {
  position: [number, number, number];
  rotation?: number; // Y-axis rotation in radians
  isOn?: boolean;
}

export function Workstation({ position, rotation = 0, isOn = false }: WorkstationProps) {
  return (
    <group position={position} rotation={[0, rotation, 0]}>
      <Desk position={[0, 0, 0]} />
      <Monitor position={[0, 0.43, -0.1]} isOn={isOn} />
    </group>
  );
}
