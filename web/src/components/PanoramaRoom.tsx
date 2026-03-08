import { useMemo } from "react";
import { useGLTF } from "@react-three/drei";
import * as THREE from "three";
import type { SemanticZone } from "../utils/panoramaSemanticMap";

const BASE = `${import.meta.env.BASE_URL}assets/kenney/mini-market/Models/GLB format/`;
const TILE_SIZE = 2;
const WALL_SCALE = 2;
const ROOM_PADDING_X = 1.4;
const ROOM_PADDING_BACK = 1.6;
const ROOM_PADDING_FRONT = 1.4;
const ROOM_TOP_Y = 4.2;

function snapDown(value: number, step: number): number {
  return Math.floor(value / step) * step;
}

function snapUp(value: number, step: number): number {
  return Math.ceil(value / step) * step;
}

function buildTileRange(from: number, to: number): number[] {
  const out: number[] = [];
  for (let value = from; value <= to; value += TILE_SIZE) out.push(value);
  return out;
}

function modelUrl(name: string): string {
  return encodeURI(`${BASE}${name}.glb`);
}

interface ModelInstanceProps {
  name: string;
  position: [number, number, number];
  rotation?: [number, number, number];
  scale?: number;
}

function ModelInstance({ name, position, rotation = [0, 0, 0], scale = 1 }: ModelInstanceProps) {
  const gltf = useGLTF(modelUrl(name));
  const object = useMemo(() => {
    const cloned = gltf.scene.clone(true);
    cloned.traverse((node) => {
      if ((node as THREE.Mesh).isMesh) {
        const mesh = node as THREE.Mesh;
        mesh.castShadow = true;
        mesh.receiveShadow = true;
      }
    });
    return cloned;
  }, [gltf.scene]);

  return <primitive object={object} position={position} rotation={rotation} scale={scale} />;
}

function RoomShell({ zones }: { zones: SemanticZone[] }) {
  const shell = useMemo(() => {
    const minX = Math.min(...zones.map((zone) => zone.center[0] - zone.size[0] / 2)) - ROOM_PADDING_X;
    const maxX = Math.max(...zones.map((zone) => zone.center[0] + zone.size[0] / 2)) + ROOM_PADDING_X;
    const minZ = Math.min(...zones.map((zone) => zone.center[2] - zone.size[1] / 2)) - ROOM_PADDING_BACK;
    const maxZ = Math.max(...zones.map((zone) => zone.center[2] + zone.size[1] / 2)) + ROOM_PADDING_FRONT;
    const leftX = snapDown(minX, TILE_SIZE);
    const rightX = snapUp(maxX, TILE_SIZE);
    const backZ = snapDown(minZ, TILE_SIZE);
    const frontZ = Math.max(backZ + TILE_SIZE * 4, snapUp(maxZ - TILE_SIZE / 2, TILE_SIZE));
    const sideWallEndZ = Math.max(backZ + TILE_SIZE * 2, frontZ - TILE_SIZE);
    const centerZ = (backZ + sideWallEndZ) / 2;
    const beamWidth = Math.max(6, rightX - leftX - 0.4);
    const lightWidth = Math.max(5.2, beamWidth - 1.4);
    return {
      leftX,
      rightX,
      backZ,
      frontZ,
      sideWallEndZ,
      centerX: (leftX + rightX) / 2,
      centerZ,
      beamWidth,
      lightWidth,
      floorXs: buildTileRange(leftX, rightX),
      floorZs: buildTileRange(backZ, frontZ),
      wallXs: buildTileRange(leftX, rightX),
      sideWallZs: buildTileRange(backZ + TILE_SIZE, sideWallEndZ),
    };
  }, [zones]);

  const floorTiles: Array<[number, number, number]> = [];
  for (const x of shell.floorXs) {
    for (const z of shell.floorZs) {
      floorTiles.push([x, 0.001, z]);
    }
  }

  return (
    <group>
      {floorTiles.map((pos, index) => (
        <ModelInstance key={`floor-${index}`} name="floor" position={pos} scale={WALL_SCALE} />
      ))}

      {shell.wallXs.map((x, index) => (
        <ModelInstance key={`back-wall-${index}`} name="wall" position={[x, 0, shell.backZ]} scale={WALL_SCALE} />
      ))}

      {shell.sideWallZs.map((z, index) => (
        <group key={`side-wall-${index}`}>
          <ModelInstance name="wall" position={[shell.leftX, 0, z]} rotation={[0, Math.PI / 2, 0]} scale={WALL_SCALE} />
          <ModelInstance name="wall" position={[shell.rightX, 0, z]} rotation={[0, -Math.PI / 2, 0]} scale={WALL_SCALE} />
        </group>
      ))}

      <ModelInstance name="wall-corner" position={[shell.leftX, 0, shell.backZ]} scale={WALL_SCALE} />
      <ModelInstance name="wall-corner" position={[shell.rightX, 0, shell.backZ]} rotation={[0, Math.PI / 2, 0]} scale={WALL_SCALE} />

      {[
        [shell.leftX, 0, shell.backZ],
        [shell.rightX, 0, shell.backZ],
        [shell.leftX, 0, shell.sideWallEndZ],
        [shell.rightX, 0, shell.sideWallEndZ],
      ].map((pos, index) => (
        <ModelInstance key={`column-${index}`} name="column" position={pos as [number, number, number]} scale={1.6} />
      ))}

      <mesh position={[shell.centerX, ROOM_TOP_Y, shell.centerZ]}>
        <boxGeometry args={[shell.beamWidth, 0.14, 0.22]} />
        <meshStandardMaterial color="#334155" emissive="#0f172a" emissiveIntensity={0.16} />
      </mesh>
      <mesh position={[shell.centerX, ROOM_TOP_Y - 0.05, shell.centerZ]}>
        <boxGeometry args={[shell.lightWidth, 0.05, 0.05]} />
        <meshStandardMaterial color="#67e8f9" emissive="#67e8f9" emissiveIntensity={0.72} />
      </mesh>
    </group>
  );
}

function ZoneProps({ zones }: { zones: SemanticZone[] }) {
  const byKind = new Map(zones.map((zone) => [zone.kind, zone]));
  const idle = byKind.get("idle");
  const offline = byKind.get("offline");
  const foreman = byKind.get("foreman");
  const taskZones = zones.filter((zone) => zone.kind === "task");

  return (
    <group>
      {idle ? (
        <ModelInstance name="shelf-end" position={[idle.center[0], 0.02, idle.center[2] + 0.55]} rotation={[0, Math.PI / 2, 0]} scale={1.02} />
      ) : null}

      {taskZones.map((zone) => (
        <ModelInstance key={zone.id} name="shelf-end" position={[zone.center[0], 0.02, zone.center[2] + 0.82]} scale={0.92} />
      ))}

      {offline ? (
        <ModelInstance name="freezer" position={[offline.center[0], 0.02, offline.center[2] + 0.66]} scale={1.08} />
      ) : null}

      {foreman ? (
        <ModelInstance name="cash-register" position={[foreman.center[0], 0.02, foreman.center[2] + 0.62]} scale={1.18} />
      ) : null}
    </group>
  );
}

export function PanoramaRoom({ zones }: { zones: SemanticZone[] }) {
  return (
    <group>
      <RoomShell zones={zones} />
      <ZoneProps zones={zones} />
    </group>
  );
}

useGLTF.preload(modelUrl("floor"));
useGLTF.preload(modelUrl("wall"));
useGLTF.preload(modelUrl("wall-corner"));
useGLTF.preload(modelUrl("column"));
useGLTF.preload(modelUrl("shelf-end"));
useGLTF.preload(modelUrl("cash-register"));
useGLTF.preload(modelUrl("freezer"));
