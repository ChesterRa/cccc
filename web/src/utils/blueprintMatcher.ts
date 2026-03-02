// Blueprint matcher: maps a Task to a blueprint via keyword rules + hash fallback

import { BLUEPRINT_IDS } from "../data/blueprints";
import type { Task } from "../types";

export interface MatchResult {
  blueprintId: string;
  variant: number; // 0-2, for future color/orientation variation
}

// Theme → blueprint mapping rules (ordered by specificity)
const THEME_RULES: { pattern: RegExp; blueprintId: string }[] = [
  // Bug fix / debugging → bug (ladybug)
  { pattern: /bug|fix|debug|error|crash|patch|hotfix|issue|defect/i, blueprintId: "bug" },
  // Refactor / optimization → gear (cog wheel)
  { pattern: /refactor|clean|optimiz|improve|perf|lint|migrat/i, blueprintId: "gear" },
  // Defense / security → shield
  { pattern: /secur|auth|permiss|guard|protect|vulnerab|sanitiz/i, blueprintId: "shield" },
  // UI / frontend → house (building structure)
  { pattern: /ui|frontend|component|page|style|css|layout|design|view|theme|form/i, blueprintId: "house" },
  // Testing → house
  { pattern: /test|spec|coverage|e2e|unit|integration|assert/i, blueprintId: "house" },
  // Documentation → book
  { pattern: /doc|readme|guide|tutorial|comment|changelog|wiki|spec/i, blueprintId: "book" },
  // New feature / creation → star (achievement)
  { pattern: /feature|add|new|create|implement|build|launch|ship/i, blueprintId: "star" },
  // Backend / API → rocket
  { pattern: /api|backend|server|database|endpoint|route|schema|model/i, blueprintId: "rocket" },
  // DevOps / deploy → rocket
  { pattern: /deploy|release|ci|cd|pipeline|docker|k8s|infra/i, blueprintId: "rocket" },
];

// Simple FNV-1a hash (no external dependency)
function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** Match a task to a blueprint ID + variant via keyword rules, with hash fallback */
export function matchBlueprint(task: Task): MatchResult {
  const text = `${task.name || ""} ${task.goal || ""}`;

  for (const rule of THEME_RULES) {
    if (rule.pattern.test(text)) {
      return {
        blueprintId: rule.blueprintId,
        variant: fnv1a(task.id) % 3,
      };
    }
  }

  // Hash fallback: deterministic but pseudo-random assignment
  const hash = fnv1a(task.id);
  return {
    blueprintId: BLUEPRINT_IDS[hash % BLUEPRINT_IDS.length],
    variant: hash % 3,
  };
}
