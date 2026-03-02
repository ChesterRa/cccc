// Behavior selector hook: picks weighted random sub-behaviors per agent per state
// Holds each selection for a minimum duration before re-rolling

import { useRef, useCallback } from "react";
import { hashCode } from "../utils/actorUtils";
import {
  type DerivedState, type BehaviorEntry, BEHAVIOR_POOLS,
} from "../data/animationProfiles";

interface AgentBehaviorState {
  behaviorId: string;
  derived: DerivedState;
  startTime: number;
  minDuration: number;
}

/** xorshift32 PRNG — deterministic per seed, never returns 0 for nonzero input */
function seededRandom(seed: number): number {
  let x = (seed | 1) >>> 0; // ensure nonzero
  x ^= x << 13;
  x ^= x >> 17;
  x ^= x << 5;
  return (x >>> 0) / 0xffffffff;
}

/** Weighted random selection from a pool */
function weightedSelect(pool: BehaviorEntry[], seed: number): BehaviorEntry {
  const total = pool.reduce((s, b) => s + b.weight, 0);
  let r = seededRandom(seed) * total;
  for (const entry of pool) {
    r -= entry.weight;
    if (r <= 0) return entry;
  }
  return pool[pool.length - 1];
}

/** Filter pool by role, fall back to full pool if nothing matches */
function filterByRole(
  pool: BehaviorEntry[],
  role: "foreman" | "worker",
): BehaviorEntry[] {
  const filtered = pool.filter((b) => !b.role || b.role === role);
  return filtered.length > 0 ? filtered : pool;
}

/**
 * Hook that manages per-agent behavior selection within each DerivedState.
 *
 * Usage (inside useFrame or similar per-frame callback):
 *   const { getBehavior } = useBehaviorSelector();
 *   const entry = getBehavior(agentId, derived, role, elapsedTime);
 *   const pose = entry.pose(ctx);
 *
 * Selection logic:
 * - On first call or when DerivedState changes → immediate new selection
 * - When minDuration elapses → re-roll with a new seed
 * - Avoids repeating the same behavior consecutively (when pool has >1 entry)
 * - Seed = hash(agentId) XOR time-based epoch → deterministic but varied
 */
export function useBehaviorSelector() {
  const stateMap = useRef<Map<string, AgentBehaviorState>>(new Map());

  const getBehavior = useCallback((
    agentId: string,
    derived: DerivedState,
    role: "foreman" | "worker",
    t: number,
  ): BehaviorEntry => {
    const rawPool = BEHAVIOR_POOLS[derived];
    if (!rawPool || rawPool.length === 0) {
      return { id: "standing", weight: 1, pose: () => ({}), minDuration: 5 };
    }

    const pool = filterByRole(rawPool, role);
    if (pool.length === 1) return pool[0];

    const hash = hashCode(agentId);
    const current = stateMap.current.get(agentId);

    // Re-select when: first call, state changed, or minDuration elapsed
    const stateChanged = !current || current.derived !== derived;
    const expired = current && (t - current.startTime) >= current.minDuration;

    if (stateChanged || expired) {
      const epoch = Math.floor(t);
      const seed = hash ^ epoch;
      let selected = weightedSelect(pool, seed);

      // Avoid repeating the same behavior consecutively
      if (current && selected.id === current.behaviorId && pool.length > 1) {
        const altSeed = seed ^ 0x9e3779b9; // golden ratio hash
        const alt = weightedSelect(pool, altSeed);
        if (alt.id !== selected.id) selected = alt;
      }

      stateMap.current.set(agentId, {
        behaviorId: selected.id,
        derived,
        startTime: t,
        minDuration: selected.minDuration,
      });
      return selected;
    }

    // Return current active behavior
    return pool.find((b) => b.id === current!.behaviorId) ?? pool[0];
  }, []);

  return { getBehavior };
}
