// Blueprint data types and pre-built blueprint definitions

export interface BlueprintBlock {
  x: number;
  y: number;
  z: number;
  color: string;
  order: number; // build order (bottom → top)
}

export interface Blueprint {
  id: string;
  name: string;
  theme: string;
  blocks: BlueprintBlock[];
  gridSize: [number, number, number]; // [maxX+1, maxY+1, maxZ+1]
  blockScale: number; // world units per block (default 0.15)
}

// Helper: auto-assign order from array index
function b(blocks: Omit<BlueprintBlock, "order">[]): BlueprintBlock[] {
  return blocks.map((bl, i) => ({ ...bl, order: i }));
}

// ── Shield (bug fix / defense theme, ~28 blocks) ──
// Flat shield shape facing -Z, blue cross emblem on silver body
const SHIELD: Blueprint = {
  id: "shield",
  name: "Shield",
  theme: "bugfix",
  blockScale: 0.15,
  gridSize: [8, 6, 2],
  blocks: b([
    // y=0 bottom point
    { x: 3, y: 0, z: 0, color: "#C0C0C0" },
    { x: 4, y: 0, z: 0, color: "#C0C0C0" },
    // y=1
    { x: 2, y: 1, z: 0, color: "#C0C0C0" },
    { x: 3, y: 1, z: 0, color: "#C0C0C0" },
    { x: 4, y: 1, z: 0, color: "#C0C0C0" },
    { x: 5, y: 1, z: 0, color: "#C0C0C0" },
    // y=2
    { x: 1, y: 2, z: 0, color: "#C0C0C0" },
    { x: 2, y: 2, z: 0, color: "#C0C0C0" },
    { x: 3, y: 2, z: 0, color: "#3B82F6" },
    { x: 4, y: 2, z: 0, color: "#3B82F6" },
    { x: 5, y: 2, z: 0, color: "#C0C0C0" },
    { x: 6, y: 2, z: 0, color: "#C0C0C0" },
    // y=3 (widest row, blue cross center)
    { x: 1, y: 3, z: 0, color: "#C0C0C0" },
    { x: 2, y: 3, z: 0, color: "#3B82F6" },
    { x: 3, y: 3, z: 0, color: "#3B82F6" },
    { x: 4, y: 3, z: 0, color: "#3B82F6" },
    { x: 5, y: 3, z: 0, color: "#3B82F6" },
    { x: 6, y: 3, z: 0, color: "#C0C0C0" },
    // y=4
    { x: 2, y: 4, z: 0, color: "#C0C0C0" },
    { x: 3, y: 4, z: 0, color: "#3B82F6" },
    { x: 4, y: 4, z: 0, color: "#3B82F6" },
    { x: 5, y: 4, z: 0, color: "#C0C0C0" },
    // y=5 top
    { x: 3, y: 5, z: 0, color: "#C0C0C0" },
    { x: 4, y: 5, z: 0, color: "#C0C0C0" },
    // z=1 depth (darker silver for 3D feel)
    { x: 3, y: 2, z: 1, color: "#A0A0A0" },
    { x: 4, y: 2, z: 1, color: "#A0A0A0" },
    { x: 3, y: 3, z: 1, color: "#A0A0A0" },
    { x: 4, y: 3, z: 1, color: "#A0A0A0" },
  ]),
};

// ── House (UI / frontend theme, ~30 blocks) ──
// Small 3D house: 3×3 footprint, oak walls, red roof
const HOUSE: Blueprint = {
  id: "house",
  name: "House",
  theme: "frontend",
  blockScale: 0.15,
  gridSize: [5, 5, 3],
  blocks: b([
    // y=0 walls (perimeter of 3×3, x=1-3 z=0-2)
    { x: 1, y: 0, z: 0, color: "#BC9862" },
    { x: 2, y: 0, z: 0, color: "#BC9862" },
    { x: 3, y: 0, z: 0, color: "#BC9862" },
    { x: 1, y: 0, z: 1, color: "#BC9862" },
    { x: 3, y: 0, z: 1, color: "#BC9862" },
    { x: 1, y: 0, z: 2, color: "#BC9862" },
    { x: 2, y: 0, z: 2, color: "#6B4423" }, // door
    { x: 3, y: 0, z: 2, color: "#BC9862" },
    // y=1 walls + windows
    { x: 1, y: 1, z: 0, color: "#BC9862" },
    { x: 2, y: 1, z: 0, color: "#3B82F6" }, // window
    { x: 3, y: 1, z: 0, color: "#BC9862" },
    { x: 1, y: 1, z: 1, color: "#BC9862" },
    { x: 3, y: 1, z: 1, color: "#BC9862" },
    { x: 1, y: 1, z: 2, color: "#BC9862" },
    { x: 2, y: 1, z: 2, color: "#3B82F6" }, // window
    { x: 3, y: 1, z: 2, color: "#BC9862" },
    // y=2 roof slab (full 3×3)
    { x: 1, y: 2, z: 0, color: "#EF4444" },
    { x: 2, y: 2, z: 0, color: "#EF4444" },
    { x: 3, y: 2, z: 0, color: "#EF4444" },
    { x: 1, y: 2, z: 1, color: "#EF4444" },
    { x: 2, y: 2, z: 1, color: "#EF4444" },
    { x: 3, y: 2, z: 1, color: "#EF4444" },
    { x: 1, y: 2, z: 2, color: "#EF4444" },
    { x: 2, y: 2, z: 2, color: "#EF4444" },
    { x: 3, y: 2, z: 2, color: "#EF4444" },
    // y=3 roof ridge
    { x: 2, y: 3, z: 0, color: "#DC2626" },
    { x: 2, y: 3, z: 1, color: "#DC2626" },
    { x: 2, y: 3, z: 2, color: "#DC2626" },
    // y=4 chimney
    { x: 3, y: 3, z: 0, color: "#8B6B3E" },
    { x: 3, y: 4, z: 0, color: "#8B6B3E" },
  ]),
};

// ── Rocket (new feature / launch theme, ~30 blocks) ──
// Vertical rocket: wide base with fins, narrow nose cone
const ROCKET: Blueprint = {
  id: "rocket",
  name: "Rocket",
  theme: "feature",
  blockScale: 0.15,
  gridSize: [5, 7, 2],
  blocks: b([
    // y=0 base + fins (z=0)
    { x: 0, y: 0, z: 0, color: "#F97316" }, // fin left
    { x: 1, y: 0, z: 0, color: "#D1D5DB" },
    { x: 2, y: 0, z: 0, color: "#D1D5DB" },
    { x: 3, y: 0, z: 0, color: "#D1D5DB" },
    { x: 4, y: 0, z: 0, color: "#F97316" }, // fin right
    // y=0 (z=1 depth)
    { x: 1, y: 0, z: 1, color: "#9CA3AF" },
    { x: 2, y: 0, z: 1, color: "#9CA3AF" },
    { x: 3, y: 0, z: 1, color: "#9CA3AF" },
    // y=1 body
    { x: 1, y: 1, z: 0, color: "#F3F4F6" },
    { x: 2, y: 1, z: 0, color: "#F3F4F6" },
    { x: 3, y: 1, z: 0, color: "#F3F4F6" },
    { x: 1, y: 1, z: 1, color: "#E5E7EB" },
    { x: 2, y: 1, z: 1, color: "#E5E7EB" },
    { x: 3, y: 1, z: 1, color: "#E5E7EB" },
    // y=2 body + window
    { x: 1, y: 2, z: 0, color: "#F3F4F6" },
    { x: 2, y: 2, z: 0, color: "#38BDF8" }, // window
    { x: 3, y: 2, z: 0, color: "#F3F4F6" },
    { x: 1, y: 2, z: 1, color: "#E5E7EB" },
    { x: 2, y: 2, z: 1, color: "#E5E7EB" },
    { x: 3, y: 2, z: 1, color: "#E5E7EB" },
    // y=3 body
    { x: 1, y: 3, z: 0, color: "#F3F4F6" },
    { x: 2, y: 3, z: 0, color: "#F3F4F6" },
    { x: 3, y: 3, z: 0, color: "#F3F4F6" },
    // y=4 red stripe
    { x: 1, y: 4, z: 0, color: "#EF4444" },
    { x: 2, y: 4, z: 0, color: "#EF4444" },
    { x: 3, y: 4, z: 0, color: "#EF4444" },
    // y=5 nose cone
    { x: 2, y: 5, z: 0, color: "#EF4444" },
    // y=3-4 depth
    { x: 2, y: 3, z: 1, color: "#E5E7EB" },
    { x: 2, y: 4, z: 1, color: "#DC2626" },
    // y=6 tip
    { x: 2, y: 6, z: 0, color: "#DC2626" },
  ]),
};

// ── Gear (refactoring / optimization theme, ~26 blocks) ──
// Cog wheel: ring with teeth at cardinal directions, gold axle center
const GEAR: Blueprint = {
  id: "gear",
  name: "Gear",
  theme: "refactor",
  blockScale: 0.15,
  gridSize: [7, 7, 2],
  blocks: b([
    // y=0 bottom tooth
    { x: 3, y: 0, z: 0, color: "#6B7280" },
    // y=1 bottom ring
    { x: 2, y: 1, z: 0, color: "#9CA3AF" },
    { x: 3, y: 1, z: 0, color: "#9CA3AF" },
    { x: 4, y: 1, z: 0, color: "#9CA3AF" },
    // y=2 ring
    { x: 1, y: 2, z: 0, color: "#9CA3AF" },
    { x: 2, y: 2, z: 0, color: "#9CA3AF" },
    { x: 4, y: 2, z: 0, color: "#9CA3AF" },
    { x: 5, y: 2, z: 0, color: "#9CA3AF" },
    // y=3 center row with teeth + axle
    { x: 0, y: 3, z: 0, color: "#6B7280" },
    { x: 1, y: 3, z: 0, color: "#9CA3AF" },
    { x: 3, y: 3, z: 0, color: "#F59E0B" },
    { x: 5, y: 3, z: 0, color: "#9CA3AF" },
    { x: 6, y: 3, z: 0, color: "#6B7280" },
    // y=4 ring
    { x: 1, y: 4, z: 0, color: "#9CA3AF" },
    { x: 2, y: 4, z: 0, color: "#9CA3AF" },
    { x: 4, y: 4, z: 0, color: "#9CA3AF" },
    { x: 5, y: 4, z: 0, color: "#9CA3AF" },
    // y=5 top ring
    { x: 2, y: 5, z: 0, color: "#9CA3AF" },
    { x: 3, y: 5, z: 0, color: "#9CA3AF" },
    { x: 4, y: 5, z: 0, color: "#9CA3AF" },
    // y=6 top tooth
    { x: 3, y: 6, z: 0, color: "#6B7280" },
    // z=1 depth (axle + cross)
    { x: 3, y: 2, z: 1, color: "#6B7280" },
    { x: 2, y: 3, z: 1, color: "#6B7280" },
    { x: 3, y: 3, z: 1, color: "#D97706" },
    { x: 4, y: 3, z: 1, color: "#6B7280" },
    { x: 3, y: 4, z: 1, color: "#6B7280" },
  ]),
};

// ── Book (documentation / guide theme, ~27 blocks) ──
// Closed book standing upright: brown spine, green cover, white pages, gold title
const BOOK: Blueprint = {
  id: "book",
  name: "Book",
  theme: "docs",
  blockScale: 0.15,
  gridSize: [4, 5, 2],
  blocks: b([
    // z=0 front face
    // y=0 bottom
    { x: 0, y: 0, z: 0, color: "#78350F" },
    { x: 1, y: 0, z: 0, color: "#166534" },
    { x: 2, y: 0, z: 0, color: "#166534" },
    // y=1
    { x: 0, y: 1, z: 0, color: "#78350F" },
    { x: 1, y: 1, z: 0, color: "#166534" },
    { x: 2, y: 1, z: 0, color: "#166534" },
    { x: 3, y: 1, z: 0, color: "#FFFBEB" },
    // y=2 with gold title band
    { x: 0, y: 2, z: 0, color: "#78350F" },
    { x: 1, y: 2, z: 0, color: "#F59E0B" },
    { x: 2, y: 2, z: 0, color: "#166534" },
    { x: 3, y: 2, z: 0, color: "#FFFBEB" },
    // y=3
    { x: 0, y: 3, z: 0, color: "#78350F" },
    { x: 1, y: 3, z: 0, color: "#166534" },
    { x: 2, y: 3, z: 0, color: "#166534" },
    { x: 3, y: 3, z: 0, color: "#FFFBEB" },
    // y=4 top
    { x: 0, y: 4, z: 0, color: "#78350F" },
    { x: 1, y: 4, z: 0, color: "#166534" },
    { x: 2, y: 4, z: 0, color: "#166534" },
    // z=1 depth
    { x: 0, y: 1, z: 1, color: "#5B2415" },
    { x: 1, y: 1, z: 1, color: "#14532D" },
    { x: 2, y: 1, z: 1, color: "#14532D" },
    { x: 0, y: 2, z: 1, color: "#5B2415" },
    { x: 1, y: 2, z: 1, color: "#14532D" },
    { x: 2, y: 2, z: 1, color: "#14532D" },
    { x: 0, y: 3, z: 1, color: "#5B2415" },
    { x: 1, y: 3, z: 1, color: "#14532D" },
    { x: 2, y: 3, z: 1, color: "#14532D" },
  ]),
};

// ── Bug (bug fix / debugging theme, ~22 blocks) ──
// Ladybug: red body with black spots, dark head, green antennae
const BUG: Blueprint = {
  id: "bug",
  name: "Bug",
  theme: "bugfix",
  blockScale: 0.15,
  gridSize: [5, 5, 2],
  blocks: b([
    // z=0 front face
    // y=0 bottom body
    { x: 1, y: 0, z: 0, color: "#EF4444" },
    { x: 2, y: 0, z: 0, color: "#EF4444" },
    { x: 3, y: 0, z: 0, color: "#EF4444" },
    // y=1 wide body with spots
    { x: 0, y: 1, z: 0, color: "#EF4444" },
    { x: 1, y: 1, z: 0, color: "#1F2937" },
    { x: 2, y: 1, z: 0, color: "#EF4444" },
    { x: 3, y: 1, z: 0, color: "#1F2937" },
    { x: 4, y: 1, z: 0, color: "#EF4444" },
    // y=2 upper body
    { x: 1, y: 2, z: 0, color: "#EF4444" },
    { x: 2, y: 2, z: 0, color: "#EF4444" },
    { x: 3, y: 2, z: 0, color: "#EF4444" },
    // y=3 head
    { x: 2, y: 3, z: 0, color: "#1F2937" },
    // y=4 antennae
    { x: 1, y: 4, z: 0, color: "#22C55E" },
    { x: 3, y: 4, z: 0, color: "#22C55E" },
    // z=1 depth (body volume)
    { x: 2, y: 0, z: 1, color: "#DC2626" },
    { x: 1, y: 1, z: 1, color: "#DC2626" },
    { x: 2, y: 1, z: 1, color: "#DC2626" },
    { x: 3, y: 1, z: 1, color: "#DC2626" },
    { x: 1, y: 2, z: 1, color: "#DC2626" },
    { x: 2, y: 2, z: 1, color: "#DC2626" },
    { x: 3, y: 2, z: 1, color: "#DC2626" },
    { x: 2, y: 3, z: 1, color: "#111827" },
  ]),
};

// ── Star (new feature / achievement theme, ~29 blocks) ──
// 5-pointed star: gold body with darker depth
const STAR: Blueprint = {
  id: "star",
  name: "Star",
  theme: "feature",
  blockScale: 0.15,
  gridSize: [7, 7, 2],
  blocks: b([
    // z=0 front face
    // bottom tips
    { x: 0, y: 0, z: 0, color: "#F59E0B" },
    { x: 6, y: 0, z: 0, color: "#F59E0B" },
    // y=1 lower legs
    { x: 1, y: 1, z: 0, color: "#F59E0B" },
    { x: 5, y: 1, z: 0, color: "#F59E0B" },
    // y=2 inner legs
    { x: 2, y: 2, z: 0, color: "#F59E0B" },
    { x: 4, y: 2, z: 0, color: "#F59E0B" },
    // y=3 lower body
    { x: 1, y: 3, z: 0, color: "#F59E0B" },
    { x: 2, y: 3, z: 0, color: "#F59E0B" },
    { x: 3, y: 3, z: 0, color: "#F59E0B" },
    { x: 4, y: 3, z: 0, color: "#F59E0B" },
    { x: 5, y: 3, z: 0, color: "#F59E0B" },
    // y=4 widest row (arms)
    { x: 0, y: 4, z: 0, color: "#F59E0B" },
    { x: 1, y: 4, z: 0, color: "#F59E0B" },
    { x: 2, y: 4, z: 0, color: "#F59E0B" },
    { x: 3, y: 4, z: 0, color: "#F59E0B" },
    { x: 4, y: 4, z: 0, color: "#F59E0B" },
    { x: 5, y: 4, z: 0, color: "#F59E0B" },
    { x: 6, y: 4, z: 0, color: "#F59E0B" },
    // y=5 upper body
    { x: 2, y: 5, z: 0, color: "#F59E0B" },
    { x: 3, y: 5, z: 0, color: "#F59E0B" },
    { x: 4, y: 5, z: 0, color: "#F59E0B" },
    // y=6 top point
    { x: 3, y: 6, z: 0, color: "#F59E0B" },
    // z=1 depth (center mass)
    { x: 3, y: 5, z: 1, color: "#D97706" },
    { x: 2, y: 4, z: 1, color: "#D97706" },
    { x: 3, y: 4, z: 1, color: "#D97706" },
    { x: 4, y: 4, z: 1, color: "#D97706" },
    { x: 2, y: 3, z: 1, color: "#D97706" },
    { x: 3, y: 3, z: 1, color: "#D97706" },
    { x: 4, y: 3, z: 1, color: "#D97706" },
  ]),
};

// ── Blueprint registry ──
export const BLUEPRINTS: Record<string, Blueprint> = {
  shield: SHIELD,
  house: HOUSE,
  rocket: ROCKET,
  gear: GEAR,
  book: BOOK,
  bug: BUG,
  star: STAR,
};

export const BLUEPRINT_IDS = Object.keys(BLUEPRINTS);
