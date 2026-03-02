// Shared build-zone grid layout constants and helpers
// Used by BuildZone (rendering) and ActorScene3D (agent walking targets)

export const BUILD_COLS = 3;
export const BUILD_SPACING = 2.5;

/** Compute world position for a grid cell by index */
export function computeGridPosition(
  index: number,
  totalCount: number,
  baseZ: number = 0,
): [number, number, number] {
  const row = Math.floor(index / BUILD_COLS);
  const col = index % BUILD_COLS;
  const rowCount = Math.min(totalCount - row * BUILD_COLS, BUILD_COLS);
  const xOffset = ((rowCount - 1) * BUILD_SPACING) / 2;
  const x = col * BUILD_SPACING - xOffset;
  const z = baseZ + row * BUILD_SPACING;
  return [x, 0, z];
}
