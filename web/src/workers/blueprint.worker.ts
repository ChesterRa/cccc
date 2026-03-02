// Blueprint validation & build-order Web Worker
// Runs off main thread to avoid blocking rendering

/** Raw block from LLM or external source (no order field) */
interface RawBlock {
  x: number;
  y: number;
  z: number;
  color: string;
}

interface WorkerRequest {
  id: string;
  type: "validate" | "computeOrder";
  payload: { blocks: RawBlock[]; gridSize?: [number, number, number] };
}

const HEX_RE = /^#[0-9a-fA-F]{3,8}$/;
const MAX_BLOCKS = 512;

/** Validate raw blocks against constraints */
function validateBlocks(
  blocks: unknown[],
  gridSize: [number, number, number],
): { valid: true; blocks: RawBlock[] } | { valid: false; error: string } {
  if (!Array.isArray(blocks) || blocks.length === 0) {
    return { valid: false, error: "blocks must be a non-empty array" };
  }
  if (blocks.length > MAX_BLOCKS) {
    return { valid: false, error: `blocks count ${blocks.length} exceeds max ${MAX_BLOCKS}` };
  }

  const seen = new Set<string>();
  const validated: RawBlock[] = [];

  for (let i = 0; i < blocks.length; i++) {
    const b = blocks[i] as Record<string, unknown>;
    if (!b || typeof b !== "object") {
      return { valid: false, error: `block[${i}]: must be an object` };
    }
    const x = b.x as number, y = b.y as number, z = b.z as number;
    if (typeof x !== "number" || typeof y !== "number" || typeof z !== "number") {
      return { valid: false, error: `block[${i}]: x/y/z must be numbers` };
    }
    if (x < 0 || x >= gridSize[0] || y < 0 || y >= gridSize[1] || z < 0 || z >= gridSize[2]) {
      return { valid: false, error: `block[${i}]: position (${x},${y},${z}) out of gridSize bounds` };
    }
    const color = b.color as string;
    if (typeof color !== "string" || !HEX_RE.test(color)) {
      return { valid: false, error: `block[${i}]: invalid hex color "${color}"` };
    }
    const key = `${x},${y},${z}`;
    if (seen.has(key)) {
      return { valid: false, error: `block[${i}]: duplicate position ${key}` };
    }
    seen.add(key);
    validated.push({ x, y, z, color });
  }

  return { valid: true, blocks: validated };
}

/** Compute build order: bottom-up (y -> x -> z) */
function computeBuildOrder(blocks: RawBlock[]): Array<RawBlock & { order: number }> {
  const sorted = [...blocks].sort((a, b) => a.y - b.y || a.x - b.x || a.z - b.z);
  return sorted.map((bl, i) => ({ ...bl, order: i }));
}

// Worker message handler
self.onmessage = (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;

  switch (msg.type) {
    case "validate": {
      const gs = msg.payload.gridSize;
      if (!gs || gs.length !== 3) {
        self.postMessage({ id: msg.id, type: "validationError", payload: { error: "gridSize must be [x,y,z]" } });
        return;
      }
      const result = validateBlocks(msg.payload.blocks, gs);
      if (result.valid) {
        const ordered = computeBuildOrder(result.blocks);
        self.postMessage({
          id: msg.id,
          type: "validated",
          payload: { blocks: ordered, gridSize: gs },
        });
      } else {
        self.postMessage({ id: msg.id, type: "validationError", payload: { error: result.error } });
      }
      break;
    }
    case "computeOrder": {
      const ordered = computeBuildOrder(msg.payload.blocks);
      self.postMessage({ id: msg.id, type: "ordered", payload: { blocks: ordered } });
      break;
    }
    default:
      self.postMessage({
        id: (msg as { id: string }).id,
        type: "error",
        payload: { error: "unknown message type" },
      });
  }
};
