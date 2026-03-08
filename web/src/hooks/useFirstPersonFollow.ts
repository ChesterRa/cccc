import { useRef } from "react";
import { useFrame, useThree } from "@react-three/fiber";
import * as THREE from "three";

interface UseFirstPersonFollowParams {
  targetId: string | null;
  characterRefs: React.MutableRefObject<Map<string, THREE.Group>>;
}

const BEHIND_DIST = 3.5;
const ABOVE_HEIGHT = 2.5;
const LOOK_HEIGHT = 0.8;

export function useFirstPersonFollow({ targetId, characterRefs }: UseFirstPersonFollowParams): void {
  const { camera } = useThree();
  const prevTargetId = useRef<string | null>(null);

  useFrame((_state, delta) => {
    if (!targetId) {
      prevTargetId.current = null;
      return;
    }

    const group = characterRefs.current.get(targetId);
    if (!group) return;

    const alpha = 1 - Math.exp(-5 * delta);

    // Character mesh faces -Z, rotation contains PI offset
    const rotY = group.rotation.y - Math.PI;
    const fwdX = Math.sin(rotY);
    const fwdZ = Math.cos(rotY);

    // Camera behind and above the character
    const camX = group.position.x - fwdX * BEHIND_DIST;
    const camY = group.position.y + ABOVE_HEIGHT;
    const camZ = group.position.z - fwdZ * BEHIND_DIST;

    // Look at character chest height
    const lookX = group.position.x;
    const lookY = group.position.y + LOOK_HEIGHT;
    const lookZ = group.position.z;

    // Snap on first frame of following a new target
    const snap = prevTargetId.current !== targetId;
    prevTargetId.current = targetId;

    if (snap) {
      camera.position.set(camX, camY, camZ);
    } else {
      camera.position.set(
        camera.position.x + (camX - camera.position.x) * alpha,
        camera.position.y + (camY - camera.position.y) * alpha,
        camera.position.z + (camZ - camera.position.z) * alpha,
      );
    }
    camera.lookAt(lookX, lookY, lookZ);
  });
}
