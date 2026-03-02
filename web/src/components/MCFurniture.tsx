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
