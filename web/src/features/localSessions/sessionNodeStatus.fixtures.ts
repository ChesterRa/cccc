import type { SessionNodeStatus } from "../../services/api/sessionNodes";

const computer = { node_id: "studio-node", name: "Studio", enabled: true };

export const pairedComputerStates = {
  unpaired: [],
  offline: [{ ...computer, connected: false }],
  online: [{ ...computer, connected: true }],
} satisfies Record<string, SessionNodeStatus["devices"]>;
