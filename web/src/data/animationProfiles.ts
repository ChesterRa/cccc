// Pure data module: animation types, constants, state transitions, pose profiles
// No React/Three.js dependencies — only plain TypeScript + Math

// ── Type Definitions ──

/** Full-body pose target (20 animatable properties) */
export interface PoseTarget {
  gX: number; gY: number; gZ: number;
  gRx: number; gRy: number; gRz: number;
  tRx: number; tPy: number;
  hRx: number; hRy: number; hPy: number;
  laRx: number; laRz: number; laPy: number;
  raRx: number; raRz: number; raPy: number;
  llRx: number; rlRx: number;
  llPy: number; rlPy: number;
}

/** Walk cycle tuning (15 knobs) */
export interface WalkProfile {
  maxMoveSpeed: number;
  strideLen: number;
  legAmp: [base: number, scale: number];
  armAmp: [base: number, scale: number];
  stepLift: [base: number, scale: number];
  lean: [base: number, scale: number];
  bob: [base: number, scale: number];
  headPitch: [base: number, scale: number];
  headYawSwing: number;
}

/** Per-frame context passed to pose computers */
export interface PoseContext {
  t: number;
  phase: number;
  variant: number;
  baseX: number; baseY: number; baseZ: number;
  charRotY: number;
  bedGoalX: number; bedGoalZ: number; bedRotY: number;
  buildTarget: [number, number, number] | null;
  patrolRadius: number;
}

export type PoseComputer = (ctx: PoseContext) => Partial<PoseTarget>;
export type GaitModifier = (wp: number, armAmp: number, isForeman: boolean) => Partial<PoseTarget>;

export type DerivedState = "offline" | "blocked" | "working" | "thinking" | "idle";

export type VisualState =
  | "sleeping_still" | "sleeping_breath"
  | "walking_to_bed"
  | "commanding" | "hammering" | "hammering_orbit"
  | "thinking_scan" | "thinking_sway"
  | "panic_pace" | "blocked_pace"
  | "standing";

export type GoalType = "bed" | "patrol" | "buildSite" | "base" | "none";

export interface StateTransitionRule {
  role: "foreman" | "worker" | "*";
  derived: DerivedState;
  atBed?: boolean;
  hasTarget?: boolean;
  staticMode?: boolean;
  visual: VisualState;
  goalType: GoalType;
  locomotion: boolean;
}

// ── Constants ──

export const BASE_Y = {
  torso: 0.55,
  head: 1.0,
  leftArm: 0.5,
  rightArm: 0.5,
  leftLeg: 0.12,
  rightLeg: 0.12,
} as const;

export const SLEEP = {
  BED_TOP_Y: 0.27,
  FACE_UP_RX: -Math.PI / 2,
  HEAD_TO_PILLOW_OFFSET: 0.65,
} as const;

export const LOCOMOTION = {
  MOVE_START_DIST: 0.16,
  MOVE_STOP_DIST: 0.05,
  AT_BED_DIST_SQ: 0.09, // 0.3²
} as const;

export const PATROL = {
  HEX_POINTS: 6,
  ANGLE_OFFSET: Math.PI / 6,
  MIN_RADIUS: 1.8,
  BUFFER: 1.2,
  SWITCH_INTERVAL: 4,
} as const;

// ── State Transition Table (15 rules) ──

export const STATE_TRANSITIONS: StateTransitionRule[] = [
  // #1-2: offline → walk to bed → sleep (process stopped)
  { role: "*", derived: "offline", atBed: false, visual: "walking_to_bed", goalType: "bed", locomotion: true },
  { role: "*", derived: "offline", atBed: true, visual: "sleeping_still", goalType: "none", locomotion: false },
  // #3: idle → stand in place (process running but no task; distinct from offline sleeping)
  { role: "*", derived: "idle", visual: "standing", goalType: "none", locomotion: false },
  // #5-7: working (always active, ignores staticMode)
  { role: "foreman", derived: "working", visual: "commanding", goalType: "patrol", locomotion: true },
  { role: "worker", derived: "working", hasTarget: true, visual: "hammering", goalType: "buildSite", locomotion: true },
  { role: "worker", derived: "working", hasTarget: false, visual: "hammering_orbit", goalType: "base", locomotion: true },
  // #8-11: thinking (static mode → standing)
  { role: "foreman", derived: "thinking", staticMode: false, visual: "thinking_scan", goalType: "base", locomotion: true },
  { role: "foreman", derived: "thinking", staticMode: true, visual: "standing", goalType: "none", locomotion: false },
  { role: "worker", derived: "thinking", staticMode: false, visual: "thinking_sway", goalType: "base", locomotion: true },
  { role: "worker", derived: "thinking", staticMode: true, visual: "standing", goalType: "none", locomotion: false },
  // #12-15: blocked (static mode → standing)
  { role: "foreman", derived: "blocked", staticMode: false, visual: "panic_pace", goalType: "base", locomotion: true },
  { role: "foreman", derived: "blocked", staticMode: true, visual: "standing", goalType: "none", locomotion: false },
  { role: "worker", derived: "blocked", staticMode: false, visual: "blocked_pace", goalType: "base", locomotion: true },
  { role: "worker", derived: "blocked", staticMode: true, visual: "standing", goalType: "none", locomotion: false },
];

const FALLBACK_RULE: StateTransitionRule = {
  role: "*", derived: "idle", visual: "standing", goalType: "none", locomotion: false,
};

/** Match agent conditions to a transition rule (first-match wins) */
export function matchTransition(
  role: "foreman" | "worker",
  derived: DerivedState,
  atBed: boolean,
  hasTarget: boolean,
  staticMode: boolean,
): StateTransitionRule {
  for (const r of STATE_TRANSITIONS) {
    if (r.role !== "*" && r.role !== role) continue;
    if (r.derived !== derived) continue;
    if (r.atBed !== undefined && r.atBed !== atBed) continue;
    if (r.hasTarget !== undefined && r.hasTarget !== hasTarget) continue;
    if (r.staticMode !== undefined && r.staticMode !== staticMode) continue;
    return r;
  }
  return FALLBACK_RULE;
}

// ── Walk Profiles ──

export const WALK_PROFILES: Record<string, WalkProfile> = {
  "working:foreman": {
    maxMoveSpeed: 1.85, strideLen: 0.76,
    legAmp: [0.2, 0.3], armAmp: [0.2, 0.24],
    stepLift: [0.04, 0.035], lean: [0.06, 0.06],
    bob: [0.01, 0.018], headPitch: [-0.03, 0.035],
    headYawSwing: 0.16,
  },
  "working:worker": {
    maxMoveSpeed: 2.25, strideLen: 0.68,
    legAmp: [0.24, 0.36], armAmp: [0.14, 0.2],
    stepLift: [0.04, 0.045], lean: [0.06, 0.06],
    bob: [0.01, 0.018], headPitch: [-0.03, 0.035],
    headYawSwing: 0.08,
  },
  thinking: {
    maxMoveSpeed: 1.1, strideLen: 0.96,
    legAmp: [0.12, 0.18], armAmp: [0.08, 0.14],
    stepLift: [0.02, 0.02], lean: [0.03, 0.03],
    bob: [0.006, 0.01], headPitch: [-0.08, 0.02],
    headYawSwing: 0.2,
  },
  blocked: {
    maxMoveSpeed: 2.45, strideLen: 0.62,
    legAmp: [0.24, 0.42], armAmp: [0.2, 0.35],
    stepLift: [0.04, 0.05], lean: [0.07, 0.08],
    bob: [0.012, 0.025], headPitch: [0.06, 0.04],
    headYawSwing: 0.26,
  },
  idle: {
    maxMoveSpeed: 0.95, strideLen: 1.02,
    legAmp: [0.1, 0.12], armAmp: [0.07, 0.1],
    stepLift: [0.015, 0.018], lean: [0.02, 0.02],
    bob: [0.004, 0.008], headPitch: [-0.02, 0.015],
    headYawSwing: 0.12,
  },
  offline: {
    maxMoveSpeed: 1.25, strideLen: 0.86,
    legAmp: [0.14, 0.2], armAmp: [0.1, 0.14],
    stepLift: [0.02, 0.03], lean: [0.03, 0.04],
    bob: [0.006, 0.012], headPitch: [-0.01, 0.02],
    headYawSwing: 0.08,
  },
};

export function getWalkProfile(derived: DerivedState, isForeman: boolean): WalkProfile {
  if (derived === "working") {
    return WALK_PROFILES[isForeman ? "working:foreman" : "working:worker"];
  }
  return WALK_PROFILES[derived] ?? WALK_PROFILES.offline;
}

// ── Pose Computers ──

function sleepPose(ctx: PoseContext, breathing: boolean): Partial<PoseTarget> {
  return {
    gX: ctx.bedGoalX,
    gZ: ctx.bedGoalZ,
    gY: SLEEP.BED_TOP_Y + (breathing ? Math.sin(ctx.t * 0.4 + ctx.phase) * 0.02 : 0),
    gRx: SLEEP.FACE_UP_RX,
    gRy: ctx.bedRotY,
  };
}

export const POSE_COMPUTERS: Record<VisualState, PoseComputer> = {
  sleeping_still: (ctx) => sleepPose(ctx, false),
  sleeping_breath: (ctx) => sleepPose(ctx, true),

  walking_to_bed: (ctx) => ({
    gX: ctx.bedGoalX,
    gZ: ctx.bedGoalZ,
    gY: 0,
  }),

  commanding: (ctx) => {
    const { t, phase, baseY, patrolRadius: pr } = ctx;
    const pts: [number, number][] = [];
    for (let p = 0; p < PATROL.HEX_POINTS; p++) {
      const a = (p / PATROL.HEX_POINTS) * Math.PI * 2 + PATROL.ANGLE_OFFSET;
      pts.push([Math.sin(a) * pr, Math.cos(a) * pr]);
    }
    const idx = Math.floor((t + phase * 5) / PATROL.SWITCH_INTERVAL) % PATROL.HEX_POINTS;
    return {
      gX: pts[idx][0], gZ: pts[idx][1], gY: baseY,
      laRx: -0.78 + Math.sin(t * 2.0 + phase) * 0.32,
      laRz: 0.26, laPy: BASE_Y.leftArm + 0.15,
      raRx: -0.45 + Math.sin(t * 1.45 + phase + 1.1) * 0.38,
      raRz: -0.25 + Math.sin(t * 0.85 + phase) * 0.24,
      raPy: BASE_Y.rightArm + 0.1,
      hRx: -0.08, hRy: Math.sin(t * 0.8 + phase) * 0.35,
    };
  },

  hammering: (ctx) => {
    const { t, phase, baseY, buildTarget } = ctx;
    if (!buildTarget) return {};
    const offsets: [number, number][] = [[0, 0.3], [0.4, -0.15], [-0.3, 0.25], [0.15, -0.3]];
    const ci = Math.floor((t + phase * 7) / 3) % offsets.length;
    const hc = Math.sin(t * 3.5 + phase);
    return {
      gX: buildTarget[0] + offsets[ci][0], gZ: buildTarget[2] + offsets[ci][1], gY: baseY,
      tRx: 0.12 + hc * 0.06, hRx: 0.06,
      laRx: -0.9 + Math.sin(t * 3.5 + phase) * 0.7,
      raRx: -0.9 + Math.sin(t * 3.5 + phase + Math.PI) * 0.7,
      laPy: BASE_Y.leftArm + 0.12, raPy: BASE_Y.rightArm + 0.12,
      llRx: Math.abs(hc) * 0.08, rlRx: Math.abs(hc) * 0.08,
    };
  },

  hammering_orbit: (ctx) => {
    const { t, phase, variant, baseX, baseY, baseZ } = ctx;
    const wp = t * (0.82 + variant * 0.08) + phase;
    const or = 0.48 + variant * 0.12;
    const hc = Math.sin(t * 3 + phase);
    return {
      gX: baseX + Math.sin(wp) * or,
      gZ: baseZ + Math.cos(wp * 0.9) * or * 0.75,
      gY: baseY + Math.abs(hc) * 0.04,
      tRx: 0.1 + hc * 0.06, hRx: 0.08,
      laRx: -0.8 + Math.sin(t * 3 + phase) * 0.8,
      raRx: -0.8 + Math.sin(t * 3 + phase + Math.PI) * 0.8,
      laPy: BASE_Y.leftArm + 0.1, raPy: BASE_Y.rightArm + 0.1,
      llRx: Math.abs(hc) * 0.1, rlRx: Math.abs(hc) * 0.1,
    };
  },

  thinking_scan: (ctx) => {
    const { t, phase, baseX, baseY, baseZ } = ctx;
    const scan = t * 0.32 + phase;
    return {
      gX: baseX + Math.sin(scan) * 0.35,
      gZ: baseZ + Math.cos(scan * 0.9) * 0.22,
      gY: baseY + Math.sin(t * 0.9 + phase) * 0.018,
      raRz: -0.68, raRx: -0.58, raPy: BASE_Y.rightArm + 0.14,
      laRx: -0.22 + Math.sin(t * 0.7 + phase) * 0.12, laRz: 0.18,
      hRx: -0.2, hRy: Math.sin(t * 0.6 + phase) * 0.42,
    };
  },

  thinking_sway: (ctx) => {
    const { t, phase, baseX, baseY, baseZ, charRotY } = ctx;
    const sway = Math.sin(t * 0.55 + phase) * 0.28;
    return {
      gX: baseX + Math.sin(charRotY) * 0.5 + sway * Math.cos(charRotY),
      gZ: baseZ - Math.cos(charRotY) * 0.5 - sway * Math.sin(charRotY),
      gY: baseY + Math.sin(t * 0.8 + phase) * 0.03,
      hRx: -0.3, hRy: Math.sin(t * 0.8 + phase) * 0.35,
      raRz: -0.7, raRx: -0.6, raPy: BASE_Y.rightArm + 0.15,
      llRx: Math.sin(t * 0.55 + phase) * 0.06,
      rlRx: Math.sin(t * 0.55 + phase + Math.PI) * 0.06,
    };
  },

  panic_pace: (ctx) => {
    const { t, phase, variant, baseX, baseY, baseZ } = ctx;
    const panic = t * (1.3 + variant * 0.15) + phase;
    return {
      gX: baseX + Math.sin(panic) * 1.15,
      gZ: baseZ + Math.cos(panic * 0.9) * 0.6,
      gY: baseY + Math.abs(Math.sin(t * 2.2 + phase)) * 0.03,
      laRz: 0.56 + Math.sin(t * 2 + phase) * 0.2,
      raRz: -0.56 - Math.sin(t * 2 + phase) * 0.2,
      laRx: -0.25 + Math.sin(t * 2 + phase) * 0.18,
      raRx: -0.3 + Math.sin(t * 2 + phase + Math.PI) * 0.18,
      laPy: BASE_Y.leftArm + 0.1, raPy: BASE_Y.rightArm + 0.1,
      hRy: Math.sin(t * 2.4 + phase) * 0.35, hRx: 0.14,
    };
  },

  blocked_pace: (ctx) => {
    const { t, phase, variant, baseX, baseY, baseZ, charRotY } = ctx;
    const pace = Math.sin(t * (2 + variant * 0.25) + phase) * 0.95;
    return {
      gX: baseX + pace * Math.cos(charRotY),
      gZ: baseZ - pace * Math.sin(charRotY),
      gY: baseY + Math.abs(Math.sin(t * 4 + phase)) * 0.04,
      hRx: 0.16, hRy: Math.sin(t * 3.2 + phase) * 0.4,
      laRx: Math.sin(t * 4.5 + phase) * 0.6,
      raRx: Math.sin(t * 4.5 + phase + Math.PI) * 0.6,
    };
  },

  standing: () => ({}),
};

// ── Gait Modifiers (walk-time state-specific overlays) ──

export const GAIT_MODIFIERS: Partial<Record<DerivedState, GaitModifier>> = {
  working: (_wp, armAmp, isForeman) => {
    if (isForeman) return {};
    return { raRx: -0.35 + Math.sin(_wp) * armAmp * 0.5, hRx: 0.02 };
  },
  thinking: () => ({ raRz: -0.25, hRx: -0.12 }),
  blocked: () => ({ laRz: 0.18, raRz: -0.18 }),
};

// ── Behavior Pool (sub-behavior variety per DerivedState) ──

/** A single weighted sub-behavior within a state's behavior pool */
export interface BehaviorEntry {
  id: string;
  weight: number;
  pose: PoseComputer;
  /** Minimum seconds before the selector may switch to another behavior */
  minDuration: number;
  /** If set, this behavior is only available for the specified role */
  role?: "foreman" | "worker";
}

// ── Phase B: Sub-Behavior Pose Functions ──
//
// Physical constraints applied throughout:
//   - Arm rotation (laRx/raRx): within ±1.047 rad (±60°)
//   - Leg rotation (llRx/rlRx): within ±0.785 rad (±45°) for standing poses
//   - Sitting: gY drops ~0.25, legs allowed up to ~1.3 rad (seated physics)
//   - laRz positive = left arm outward; raRz negative = right arm outward
//   - Animation frequencies: 0.25–4.0 Hz via Math.sin(t * freq + phase)

// --- idle sub-behaviors ---

/** Stretch — arms raise overhead, slight back lean, periodic breathing cycle */
const stretchPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const cycle = Math.sin(t * 0.3 + phase);
  return {
    gX: baseX, gZ: baseZ, gY: baseY,
    laRx: -0.85 + cycle * 0.15,   // left arm up: 40°–57°
    raRx: -0.9 + cycle * 0.1,     // right arm up: 46°–57°
    laRz: 0.35 + cycle * 0.1,     // spread outward
    raRz: -0.35 - cycle * 0.1,
    laPy: BASE_Y.leftArm + 0.05,
    raPy: BASE_Y.rightArm + 0.05,
    tRx: -0.08 + cycle * 0.03,    // slight back lean
    hRx: -0.15 + cycle * 0.05,    // head tilts back
  };
};

/** Look around — head turns left/right, subtle body weight shift */
const lookAroundPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const headTurn = Math.sin(t * 0.7 + phase);
  return {
    gX: baseX + Math.sin(t * 0.35 + phase) * 0.05,
    gZ: baseZ, gY: baseY,
    hRy: headTurn * 0.5,                              // head yaw ±29°
    hRx: Math.sin(t * 0.5 + phase + 1) * 0.08,       // gentle nod
    tRx: 0.02,
    laRx: Math.sin(t * 0.4 + phase) * 0.06,           // relaxed sway
    raRx: Math.sin(t * 0.4 + phase + 1.5) * 0.06,
  };
};

/**
 * Sit down — body drops ~0.25 (hip height), legs forward, arms rest on thighs.
 * Gravity: gY lowered so character doesn't float.
 * Legs at ~1.3 rad (75°) simulates MC-style seated pose.
 */
const sitDownPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const breathe = Math.sin(t * 0.5 + phase) * 0.015;
  return {
    gX: baseX, gZ: baseZ,
    gY: baseY - 0.25,                // KEY: hip drops for sitting
    tRx: 0.15,                       // lean forward
    tPy: BASE_Y.torso - 0.1,
    hRx: 0.05 + breathe,             // head slightly down, breathing
    hRy: Math.sin(t * 0.3 + phase) * 0.15,
    hPy: BASE_Y.head - 0.1,
    llRx: 1.3, rlRx: 1.3,           // legs forward (seated)
    llPy: BASE_Y.leftLeg + 0.05,
    rlPy: BASE_Y.rightLeg + 0.05,
    laRx: 0.35 + breathe,           // arms on thighs ~20°
    raRx: 0.35 + breathe,
    laPy: BASE_Y.leftArm - 0.08,
    raPy: BASE_Y.rightArm - 0.08,
  };
};

// --- thinking sub-behaviors ---

/** Chin rub — right hand raised to chin, thoughtful nodding */
const chinRubPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const rub = Math.sin(t * 2.5 + phase) * 0.08;
  return {
    gX: baseX, gZ: baseZ, gY: baseY,
    raRx: -0.85 + rub,              // right arm to chin: 44°–53°
    raRz: -0.2,
    raPy: BASE_Y.rightArm + 0.2,
    laRx: -0.3,                     // left arm relaxed
    laRz: 0.15,
    hRx: -0.12 + rub * 0.3,         // nod with rub motion
    hRy: Math.sin(t * 0.4 + phase) * 0.1,
    tRx: 0.05,
  };
};

/** Pace — walk back and forth in a small line, arms behind back */
const pacePose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ, charRotY } = ctx;
  const paceOffset = Math.sin(t * 0.8 + phase) * 0.6;
  return {
    gX: baseX + paceOffset * Math.cos(charRotY),
    gZ: baseZ - paceOffset * Math.sin(charRotY),
    gY: baseY,
    llRx: Math.sin(t * 1.6 + phase) * 0.25,           // walking legs
    rlRx: Math.sin(t * 1.6 + phase + Math.PI) * 0.25,
    laRx: 0.3,                                          // arms behind back ~17°
    raRx: 0.3,
    laRz: 0.1, raRz: -0.1,
    hRx: -0.1,                                          // head slightly down
    hRy: Math.sin(t * 0.5 + phase) * 0.15,
    tRx: 0.04,
  };
};

/** Look up — head tilted back, gazing skyward with slow scan */
const lookUpPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const sway = Math.sin(t * 0.25 + phase);
  return {
    gX: baseX, gZ: baseZ, gY: baseY,
    hRx: -0.45 + sway * 0.08,      // head back ~21°–30°
    hRy: sway * 0.2,               // slow sky scan
    tRx: -0.06,                    // slight lean back
    laRx: 0.05 + sway * 0.03,     // arms hang naturally
    raRx: 0.05 - sway * 0.03,
  };
};

// --- working sub-behaviors ---

/** Focused single-arm hammering — right arm dominant, knee-bend on impact */
const hammerPose: PoseComputer = (ctx) => {
  const { t, phase, baseY, buildTarget, baseX, baseZ } = ctx;
  const tx = buildTarget ? buildTarget[0] : baseX;
  const tz = buildTarget ? buildTarget[2] : baseZ;
  const hc = Math.sin(t * 4 + phase);
  return {
    gX: tx, gZ: tz, gY: baseY,
    tRx: 0.08 + Math.abs(hc) * 0.05, // subtle lean into work
    hRx: 0.05,                        // looking at work
    raRx: -0.6 + hc * 0.4,           // hammer swing: 11°–57°
    raRz: -0.15,
    raPy: BASE_Y.rightArm + 0.1,
    laRx: -0.4,                       // brace arm ~23°
    laRz: 0.2,
    laPy: BASE_Y.leftArm + 0.05,
    llRx: Math.abs(hc) * 0.06,       // knee bend on impact
    rlRx: Math.abs(hc) * 0.06,
  };
};

/** Carry — both arms forward holding material, slight lean back */
const carryPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ, buildTarget } = ctx;
  const tx = buildTarget ? buildTarget[0] : baseX;
  const tz = buildTarget ? buildTarget[2] : baseZ;
  const wobble = Math.sin(t * 1.5 + phase) * 0.03;
  return {
    gX: tx, gZ: tz,
    gY: baseY + wobble,               // effort wobble
    tRx: -0.06,                       // lean back (counterweight)
    laRx: -0.7, raRx: -0.7,          // arms forward ~40°
    laRz: 0.15, raRz: -0.15,         // arms inward
    laPy: BASE_Y.leftArm + 0.08,
    raPy: BASE_Y.rightArm + 0.08,
    hRx: 0.08,                        // looking at carried item
    llRx: Math.sin(t * 1.2 + phase) * 0.08,
    rlRx: Math.sin(t * 1.2 + phase + Math.PI) * 0.08,
  };
};

/** Measure — right arm extended pointing, left hand on hip */
const measurePose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ, buildTarget } = ctx;
  const tx = buildTarget ? buildTarget[0] : baseX;
  const tz = buildTarget ? buildTarget[2] : baseZ;
  const scan = Math.sin(t * 0.3 + phase);
  return {
    gX: tx, gZ: tz, gY: baseY,
    raRx: -0.85 + scan * 0.1,        // measuring arm: 43°–54°
    raRz: -0.1,
    raPy: BASE_Y.rightArm + 0.12,
    laRx: 0.2,                        // left hand on hip
    laRz: 0.45,
    hRx: -0.05,                       // squinting along arm
    hRy: scan * 0.2,
    tRx: 0.06,
  };
};

// --- blocked sub-behaviors ---

/** Scratch head — confused, right arm raised to head */
const scratchHeadPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const scratch = Math.sin(t * 3 + phase) * 0.08;
  return {
    gX: baseX, gZ: baseZ, gY: baseY,
    raRx: -0.88 + scratch,            // arm to head: 46°–55° (safety margin from 60° limit)
    raRz: -0.3,
    raPy: BASE_Y.rightArm + 0.25,
    laRx: 0.05 + Math.sin(t * 0.5 + phase) * 0.05, // left arm hangs
    hRx: 0.08 + scratch * 0.5,        // head jiggles with scratch
    hRy: Math.sin(t * 0.6 + phase) * 0.2,
    tRx: 0.02,
    llRx: Math.sin(t * 0.4 + phase) * 0.04,  // weight shift
    rlRx: -Math.sin(t * 0.4 + phase) * 0.04,
  };
};

/** Hands on hips — impatient standing, side-to-side weight shift */
const handsOnHipsPose: PoseComputer = (ctx) => {
  const { t, phase, baseX, baseY, baseZ } = ctx;
  const shift = Math.sin(t * 0.5 + phase) * 0.06;
  return {
    gX: baseX + shift, gZ: baseZ, gY: baseY,
    laRx: 0.15, laRz: 0.55,          // elbows out (hands on hips)
    raRx: 0.15, raRz: -0.55,
    hRx: 0.05,
    hRy: Math.sin(t * 0.8 + phase) * 0.25, // impatient looking around
    tRx: 0.04,
    llRx: shift * 0.3,               // weight shift
    rlRx: -shift * 0.3,
  };
};

// ── Behavior Pools ──

/**
 * Per-state behavior pools with weighted sub-behaviors.
 * The BehaviorSelector picks one at random (weighted), holds it for at least
 * `minDuration` seconds, then re-rolls.
 */
export const BEHAVIOR_POOLS: Record<DerivedState, BehaviorEntry[]> = {
  offline: [
    { id: "sleeping_still", weight: 1, pose: POSE_COMPUTERS.sleeping_still, minDuration: 10 },
  ],
  idle: [
    { id: "sleeping_breath", weight: 3, pose: POSE_COMPUTERS.sleeping_breath, minDuration: 8 },
    { id: "stretch",    weight: 1, pose: stretchPose,    minDuration: 4 },
    { id: "lookAround", weight: 1, pose: lookAroundPose, minDuration: 5 },
    { id: "sitDown",    weight: 1, pose: sitDownPose,    minDuration: 6 },
  ],
  thinking: [
    { id: "thinking_scan", weight: 2, pose: POSE_COMPUTERS.thinking_scan, minDuration: 6, role: "foreman" },
    { id: "thinking_sway", weight: 2, pose: POSE_COMPUTERS.thinking_sway, minDuration: 6, role: "worker" },
    { id: "chinRub", weight: 1, pose: chinRubPose, minDuration: 5 },
    { id: "pace",    weight: 1, pose: pacePose,    minDuration: 5 },
    { id: "lookUp",  weight: 1, pose: lookUpPose,  minDuration: 4 },
  ],
  working: [
    { id: "commanding",      weight: 2, pose: POSE_COMPUTERS.commanding,      minDuration: 6, role: "foreman" },
    { id: "hammering",       weight: 2, pose: POSE_COMPUTERS.hammering,       minDuration: 4, role: "worker" },
    { id: "hammering_orbit", weight: 1, pose: POSE_COMPUTERS.hammering_orbit, minDuration: 4, role: "worker" },
    { id: "hammer",  weight: 2, pose: hammerPose,  minDuration: 4, role: "worker" },
    { id: "carry",   weight: 1, pose: carryPose,   minDuration: 5, role: "worker" },
    { id: "measure", weight: 1, pose: measurePose, minDuration: 5, role: "worker" },
  ],
  blocked: [
    { id: "panic_pace",   weight: 1, pose: POSE_COMPUTERS.panic_pace,   minDuration: 5, role: "foreman" },
    { id: "blocked_pace", weight: 1, pose: POSE_COMPUTERS.blocked_pace, minDuration: 5, role: "worker" },
    { id: "scratchHead",  weight: 1, pose: scratchHeadPose,  minDuration: 4 },
    { id: "handsOnHips",  weight: 1, pose: handsOnHipsPose,  minDuration: 5 },
  ],
};
