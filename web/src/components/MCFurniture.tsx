import React from "react";
import * as THREE from "three";

// ── MC Grass Block ground (green top, brown sides) ──
const GROUND_GEO = new THREE.BoxGeometry(20, 0.5, 20);
const GRASS_TOP = new THREE.MeshStandardMaterial({ color: "#5B8731", flatShading: true });
const DIRT_SIDE = new THREE.MeshStandardMaterial({ color: "#8B6B3E", flatShading: true });
// Material array order: [+X, -X, +Y, -Y, +Z, -Z]
const GROUND_MATS = [DIRT_SIDE, DIRT_SIDE, GRASS_TOP, DIRT_SIDE, DIRT_SIDE, DIRT_SIDE];

export function MCGround() {
  return <mesh position={[0, -0.25, 0]} receiveShadow geometry={GROUND_GEO} material={GROUND_MATS} />;
}

// ── MC Bed (red blanket + wood frame + pillow) ──
const BED_FRAME_GEO = new THREE.BoxGeometry(0.5, 0.12, 1.0);
const BED_BLANKET_GEO = new THREE.BoxGeometry(0.46, 0.05, 0.7);
const BED_PILLOW_GEO = new THREE.BoxGeometry(0.32, 0.07, 0.22);
const BED_WOOD = new THREE.MeshStandardMaterial({ color: "#8B6B3E", flatShading: true });
const BED_RED = new THREE.MeshStandardMaterial({ color: "#B02020", flatShading: true });
const BED_WHITE = new THREE.MeshStandardMaterial({ color: "#E8E0D0", flatShading: true });

interface MCBedProps {
  position: [number, number, number];
  rotationY: number;
}

export function MCBed({ position, rotationY }: MCBedProps) {
  return (
    <group position={position} rotation={[0, rotationY, 0]}>
      <mesh position={[0, 0.06, 0]} geometry={BED_FRAME_GEO} material={BED_WOOD} castShadow receiveShadow />
      <mesh position={[0, 0.145, 0.1]} geometry={BED_BLANKET_GEO} material={BED_RED} />
      <mesh position={[0, 0.155, -0.35]} geometry={BED_PILLOW_GEO} material={BED_WHITE} />
    </group>
  );
}

// ── Instanced Beds (single draw call per sub-mesh) ──
// Replaces N × MCBed with 3 InstancedMesh (frame/blanket/pillow)

const MAX_BED_INSTANCES = 64;

// Scratch objects for world matrix computation (synchronous use only)
const _parent = new THREE.Object3D();
const _child = new THREE.Object3D();
const _mat4 = new THREE.Matrix4();

// Sub-mesh local offsets (matching MCBed's child positions)
const BED_PARTS = [
  { geo: BED_FRAME_GEO,   mat: BED_WOOD,  offset: [0, 0.06, 0] as const,    castShadow: true, receiveShadow: true },
  { geo: BED_BLANKET_GEO, mat: BED_RED,    offset: [0, 0.145, 0.1] as const, castShadow: false, receiveShadow: false },
  { geo: BED_PILLOW_GEO,  mat: BED_WHITE,  offset: [0, 0.155, -0.35] as const, castShadow: false, receiveShadow: false },
] as const;

export interface BedInstance {
  position: [number, number, number];
  rotationY: number;
}

interface InstancedBedsProps {
  beds: BedInstance[];
}

export function InstancedBeds({ beds }: InstancedBedsProps) {
  const frameRef = React.useRef<THREE.InstancedMesh>(null);
  const blanketRef = React.useRef<THREE.InstancedMesh>(null);
  const pillowRef = React.useRef<THREE.InstancedMesh>(null);

  React.useEffect(() => {
    const refs = [frameRef.current, blanketRef.current, pillowRef.current];
    const n = Math.min(beds.length, MAX_BED_INSTANCES);

    for (let i = 0; i < n; i++) {
      const bed = beds[i];
      // Parent transform: position + Y rotation
      _parent.position.set(bed.position[0], bed.position[1], bed.position[2]);
      _parent.rotation.set(0, bed.rotationY, 0);
      _parent.updateMatrix();

      for (let p = 0; p < BED_PARTS.length; p++) {
        const mesh = refs[p];
        if (!mesh) continue;
        const part = BED_PARTS[p];
        // Child local offset
        _child.position.set(part.offset[0], part.offset[1], part.offset[2]);
        _child.rotation.set(0, 0, 0);
        _child.scale.setScalar(1);
        _child.updateMatrix();
        // World matrix = parent × child
        _mat4.multiplyMatrices(_parent.matrix, _child.matrix);
        mesh.setMatrixAt(i, _mat4);
      }
    }

    for (const mesh of refs) {
      if (!mesh) continue;
      mesh.count = n;
      mesh.instanceMatrix.needsUpdate = true;
    }
  }, [beds]);

  return (
    <>
      <instancedMesh ref={frameRef} args={[BED_FRAME_GEO, BED_WOOD, MAX_BED_INSTANCES]} castShadow receiveShadow />
      <instancedMesh ref={blanketRef} args={[BED_BLANKET_GEO, BED_RED, MAX_BED_INSTANCES]} />
      <instancedMesh ref={pillowRef} args={[BED_PILLOW_GEO, BED_WHITE, MAX_BED_INSTANCES]} />
    </>
  );
}
