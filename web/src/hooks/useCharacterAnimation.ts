// Character animation hook: drives all agent poses + locomotion each frame
// Uses declarative state transition table from animationProfiles

import { useRef } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { hashCode, deriveAnimState, PART_INDEX } from "../utils/actorUtils";
import {
  type PoseTarget, type PoseContext, type DerivedState,
  BASE_Y, SLEEP, LOCOMOTION, PATROL,
  matchTransition, POSE_COMPUTERS, getWalkProfile, GAIT_MODIFIERS,
} from "../data/animationProfiles";
import type { AgentState, Actor } from "../types";

export interface LayoutItem {
  agentId: string;
  charPos: [number, number, number];
  charRotY: number;
  bedPos: [number, number, number];
  bedRotY: number;
  isForeman: boolean;
}

interface LocomotionState {
  moving: boolean;
  speed: number;
  stridePhase: number;
}

export interface UseCharacterAnimationParams {
  agents: AgentState[];
  actorMap: Map<string, Actor>;
  layout: Map<string, LayoutItem>;
  buildTargetMap: Map<string, [number, number, number]>;
  characterRefs: React.MutableRefObject<Map<string, THREE.Group>>;
  staticMode?: boolean;
}

function lerpAngle(current: number, target: number, alpha: number): number {
  const twoPi = Math.PI * 2;
  let delta = (target - current) % twoPi;
  if (delta > Math.PI) delta -= twoPi;
  if (delta < -Math.PI) delta += twoPi;
  return current + delta * alpha;
}

export function useCharacterAnimation({
  agents, actorMap, layout, buildTargetMap, characterRefs, staticMode = true,
}: UseCharacterAnimationParams): void {
  const locomotionRefs = useRef<Map<string, LocomotionState>>(new Map());
  // Pulse animation state per agent (for status bubble sprite)
  const pulseRefs = useRef<Map<string, { prevState: string; pulse: number; baseW: number; baseH: number }>>(new Map());

  useFrame((state, delta) => {
    const t = state.clock.elapsedTime;
    const lf = 1 - Math.exp(-6 * delta);
    const lfFast = 1 - Math.exp(-10 * delta);

    // Shared patrol radius from build zone extent
    let patrolRadius: number = PATROL.MIN_RADIUS;
    for (const [, pos] of buildTargetMap) {
      const d = Math.sqrt(pos[0] * pos[0] + pos[2] * pos[2]);
      patrolRadius = Math.max(patrolRadius, d + PATROL.BUFFER);
    }

    for (const agent of agents) {
      const group = characterRefs.current.get(agent.id);
      if (!group || group.children.length < 6) continue;

      const item = layout.get(agent.id);
      if (!item) continue;

      const phase = hashCode(agent.id) * 0.1;
      const variant = hashCode(agent.id) % 3;
      const actor = actorMap.get(agent.id);
      const running = actor?.running !== false && actor?.enabled !== false;
      const derived = deriveAnimState(agent, running) as DerivedState;

      // Bed arrival detection
      const bedDirX = Math.sin(item.bedRotY);
      const bedDirZ = Math.cos(item.bedRotY);
      const bedGoalX = item.bedPos[0] + bedDirX * SLEEP.HEAD_TO_PILLOW_OFFSET;
      const bedGoalZ = item.bedPos[2] + bedDirZ * SLEEP.HEAD_TO_PILLOW_OFFSET;
      const toBedSq = (bedGoalX - group.position.x) ** 2 + (bedGoalZ - group.position.z) ** 2;
      const atBed = toBedSq < LOCOMOTION.AT_BED_DIST_SQ;

      const buildTarget = buildTargetMap.get(agent.id) ?? null;
      const role: "foreman" | "worker" = item.isForeman ? "foreman" : "worker";

      // ── State transition lookup ──
      const rule = matchTransition(role, derived, atBed, buildTarget !== null, staticMode);

      // ── Pose computation ──
      const ctx: PoseContext = {
        t, phase, variant,
        baseX: item.charPos[0], baseY: item.charPos[1], baseZ: item.charPos[2],
        charRotY: item.charRotY,
        bedGoalX, bedGoalZ, bedRotY: item.bedRotY,
        buildTarget, patrolRadius,
      };

      const overrides = POSE_COMPUTERS[rule.visual](ctx);
      const pose: PoseTarget = {
        gX: ctx.baseX, gY: ctx.baseY, gZ: ctx.baseZ,
        gRx: 0, gRy: item.charRotY, gRz: 0,
        tRx: 0, tPy: BASE_Y.torso,
        hRx: 0, hRy: 0, hPy: BASE_Y.head,
        laRx: 0, laRz: 0, laPy: BASE_Y.leftArm,
        raRx: 0, raRz: 0, raPy: BASE_Y.rightArm,
        llRx: 0, rlRx: 0,
        llPy: BASE_Y.leftLeg, rlPy: BASE_Y.rightLeg,
        ...overrides,
      };

      // ── Locomotion ──
      let loco = locomotionRefs.current.get(agent.id);
      if (!loco) {
        loco = { moving: false, speed: 0, stridePhase: 0 };
        locomotionRefs.current.set(agent.id, loco);
      }

      const dx = pose.gX - group.position.x;
      const dz = pose.gZ - group.position.z;
      const dist = Math.sqrt(dx * dx + dz * dz);

      const wp = getWalkProfile(derived, item.isForeman);
      const shouldMove = rule.locomotion && (
        dist > LOCOMOTION.MOVE_START_DIST || (loco.moving && dist > LOCOMOTION.MOVE_STOP_DIST)
      );
      loco.moving = shouldMove;

      let posLerp = lf;
      if (shouldMove) {
        const invDist = dist > 1e-6 ? 1 / dist : 0;
        const dirX = dx * invDist;
        const dirZ = dz * invDist;
        const step = Math.min(dist, wp.maxMoveSpeed * delta);
        const instSpeed = step / Math.max(delta, 1e-6);

        pose.gX = group.position.x + dirX * step;
        pose.gZ = group.position.z + dirZ * step;
        pose.gY = 0;
        pose.gRx = 0;
        pose.gRz = 0;
        // Character mesh faces -Z, so add PI so face points toward movement
        pose.gRy = Math.atan2(dirX, dirZ) + Math.PI;
        posLerp = 1;

        loco.speed = THREE.MathUtils.lerp(loco.speed, instSpeed, lfFast);
        const speedNorm = THREE.MathUtils.clamp(loco.speed / wp.maxMoveSpeed, 0, 1);
        loco.stridePhase += (step / wp.strideLen) * Math.PI * 2;

        const sp = loco.stridePhase;
        const legAmp = wp.legAmp[0] + speedNorm * wp.legAmp[1];
        const armAmp = wp.armAmp[0] + speedNorm * wp.armAmp[1];
        const stepLift = wp.stepLift[0] + speedNorm * wp.stepLift[1];
        const bob = Math.abs(Math.sin(sp * 2)) * (wp.bob[0] + speedNorm * wp.bob[1]);

        pose.llRx = Math.sin(sp) * legAmp;
        pose.rlRx = Math.sin(sp + Math.PI) * legAmp;
        pose.llPy = BASE_Y.leftLeg + Math.max(0, Math.sin(sp)) * stepLift;
        pose.rlPy = BASE_Y.rightLeg + Math.max(0, Math.sin(sp + Math.PI)) * stepLift;

        pose.laRx = Math.sin(sp + Math.PI) * armAmp;
        pose.raRx = Math.sin(sp) * armAmp;
        pose.laRz = 0;
        pose.raRz = 0;
        pose.laPy = BASE_Y.leftArm;
        pose.raPy = BASE_Y.rightArm;

        pose.tRx = wp.lean[0] + speedNorm * wp.lean[1];
        pose.tPy = BASE_Y.torso + bob;
        pose.hRx = wp.headPitch[0] - speedNorm * wp.headPitch[1];
        pose.hRy = Math.sin(sp * 0.5 + phase) * wp.headYawSwing;
        pose.hPy = BASE_Y.head + bob;

        // Gait modifier: state-specific walk overlay
        const gaitMod = GAIT_MODIFIERS[derived];
        if (gaitMod) Object.assign(pose, gaitMod(sp, armAmp, item.isForeman));
      } else {
        loco.speed = THREE.MathUtils.lerp(loco.speed, 0, lfFast);
      }

      // Face build target when arrived (workers only)
      // Character mesh faces -Z, so add PI to atan2 result
      if (!shouldMove && buildTarget && !item.isForeman) {
        pose.gRy = Math.atan2(
          buildTarget[0] - pose.gX,
          buildTarget[2] - pose.gZ,
        ) + Math.PI;
      }
      // Foreman faces scene center when stationary
      if (!shouldMove && item.isForeman) {
        pose.gRy = Math.atan2(
          0 - pose.gX,
          0 - pose.gZ,
        ) + Math.PI;
      }

      // Compensate arm/leg rotation for pivot shift from geometry center to joint:
      // joint-to-tip distance doubled → halve and negate rotation (translate flips visual direction).
      pose.laRx *= -0.5;
      pose.raRx *= -0.5;
      pose.laRz *= -0.5;
      pose.raRz *= -0.5;
      pose.llRx *= -0.5;
      pose.rlRx *= -0.5;

      // ── Apply lerp to all body parts ──
      const torso = group.children[PART_INDEX.torso];
      const head = group.children[PART_INDEX.head];
      const lArm = group.children[PART_INDEX.leftArm];
      const rArm = group.children[PART_INDEX.rightArm];
      const lLeg = group.children[PART_INDEX.leftLeg];
      const rLeg = group.children[PART_INDEX.rightLeg];

      const L = THREE.MathUtils.lerp;
      group.position.x = L(group.position.x, pose.gX, posLerp);
      group.position.y = L(group.position.y, pose.gY, posLerp);
      group.position.z = L(group.position.z, pose.gZ, posLerp);
      group.rotation.x = L(group.rotation.x, pose.gRx, lf);
      group.rotation.y = lerpAngle(group.rotation.y, pose.gRy, lfFast);
      group.rotation.z = L(group.rotation.z, pose.gRz, lf);
      torso.rotation.x = L(torso.rotation.x, pose.tRx, lf);
      torso.position.y = L(torso.position.y, pose.tPy, lf);
      head.rotation.x = L(head.rotation.x, pose.hRx, lf);
      head.rotation.y = L(head.rotation.y, pose.hRy, lf);
      head.position.y = L(head.position.y, pose.hPy, lf);

      // Crown (child 6, foreman only) follows head bob
      if (item.isForeman && group.children.length > 6) {
        const crown = group.children[6];
        const crownBaseY = 1.165; // matches ActorCharacter crown position
        crown.position.y = L(crown.position.y, crownBaseY + (pose.hPy - BASE_Y.head), lf);
      }

      lArm.rotation.x = L(lArm.rotation.x, pose.laRx, lf);
      lArm.rotation.z = L(lArm.rotation.z, pose.laRz, lf);
      lArm.position.y = L(lArm.position.y, pose.laPy, lf);
      rArm.rotation.x = L(rArm.rotation.x, pose.raRx, lf);
      rArm.rotation.z = L(rArm.rotation.z, pose.raRz, lf);
      rArm.position.y = L(rArm.position.y, pose.raPy, lf);
      lLeg.rotation.x = L(lLeg.rotation.x, pose.llRx, lf);
      lLeg.position.y = L(lLeg.position.y, pose.llPy, lf);
      rLeg.rotation.x = L(rLeg.rotation.x, pose.rlRx, lf);
      rLeg.position.y = L(rLeg.position.y, pose.rlPy, lf);

      // Sprite pulse animation (status bubble is always last child)
      const lastChild = group.children[group.children.length - 1];
      if (lastChild && (lastChild as THREE.Sprite).isSprite) {
        let ps = pulseRefs.current.get(agent.id);
        if (!ps) {
          // Capture base scale from declarative JSX on first encounter
          ps = { prevState: derived, pulse: 0, baseW: lastChild.scale.x, baseH: lastChild.scale.y };
          pulseRefs.current.set(agent.id, ps);
        }
        // Detect base scale change (texture rebuild changes dimensions)
        if (ps.pulse <= 0) {
          ps.baseW = lastChild.scale.x;
          ps.baseH = lastChild.scale.y;
        }
        if (ps.prevState !== derived) {
          ps.prevState = derived;
          ps.pulse = 1.0;
        }
        if (ps.pulse > 0) {
          ps.pulse = Math.max(0, ps.pulse - delta * 3.3);
          const boost = Math.sin(ps.pulse * Math.PI) * 0.15;
          lastChild.scale.set(ps.baseW * (1 + boost), ps.baseH * (1 + boost), 1);
        }
      }
    }
  });
}
