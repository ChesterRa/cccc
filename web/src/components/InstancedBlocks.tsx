// InstancedMesh-based block renderer: single draw call for all blocks
// Supports per-instance color via instanceColor attribute

import { forwardRef, useRef, useMemo, useEffect, useImperativeHandle } from "react";
import * as THREE from "three";

const UNIT_BOX = new THREE.BoxGeometry(1, 1, 1);
const MAX_INSTANCES = 64;

// Scratch objects for matrix/color computation (synchronous use only)
const _obj = new THREE.Object3D();
const _col = new THREE.Color();

export interface BlockInstance {
  position: [number, number, number];
  color: string;
  scale: number;
}

export interface InstancedBlocksHandle {
  /** Update a single block's transform + color (for animation, called from useFrame) */
  setBlock(index: number, position: [number, number, number], color: string, scale: number): void;
  /** Flush pending matrix/color updates to GPU */
  flush(): void;
}

interface InstancedBlocksProps {
  blocks: BlockInstance[];
  opacity?: number;
  transparent?: boolean;
}

export const InstancedBlocks = forwardRef<InstancedBlocksHandle, InstancedBlocksProps>(
  function InstancedBlocks({ blocks, opacity = 1, transparent = false }, ref) {
    const meshRef = useRef<THREE.InstancedMesh>(null);

    const mat = useMemo(
      () => new THREE.MeshStandardMaterial({ flatShading: true, transparent, opacity }),
      [opacity, transparent],
    );
    useEffect(() => () => { mat.dispose(); }, [mat]);

    // Bulk sync all instances when blocks array changes
    useEffect(() => {
      const mesh = meshRef.current;
      if (!mesh) return;
      const n = Math.min(blocks.length, MAX_INSTANCES);
      for (let i = 0; i < n; i++) {
        const b = blocks[i];
        _obj.position.set(b.position[0], b.position[1], b.position[2]);
        _obj.scale.setScalar(b.scale);
        _obj.updateMatrix();
        mesh.setMatrixAt(i, _obj.matrix);
        mesh.setColorAt(i, _col.set(b.color));
      }
      mesh.count = n;
      mesh.instanceMatrix.needsUpdate = true;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    }, [blocks]);

    // Imperative handle for per-block animation updates
    useImperativeHandle(ref, () => ({
      setBlock(index, position, color, scale) {
        const mesh = meshRef.current;
        if (!mesh || index >= MAX_INSTANCES) return;
        _obj.position.set(position[0], position[1], position[2]);
        _obj.scale.setScalar(scale);
        _obj.updateMatrix();
        mesh.setMatrixAt(index, _obj.matrix);
        mesh.setColorAt(index, _col.set(color));
      },
      flush() {
        const mesh = meshRef.current;
        if (!mesh) return;
        mesh.instanceMatrix.needsUpdate = true;
        if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
      },
    }));

    return (
      <instancedMesh
        ref={meshRef}
        args={[UNIT_BOX, mat, MAX_INSTANCES]}
        castShadow={!transparent}
        receiveShadow={!transparent}
      />
    );
  },
);
