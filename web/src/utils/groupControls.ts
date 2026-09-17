import type { GroupStatusKey } from "./groupStatus";

export type GroupRunAction = "start" | "resume" | "pause" | "stop";
export type PendingGroupAction = { groupId: string; action: GroupRunAction };
export interface GroupRunControls {
  pending: PendingGroupAction | null;
  run: (groupId: string, action: GroupRunAction) => Promise<void>;
}

export function groupRunActions(status: GroupStatusKey): GroupRunAction[] {
  switch (status) {
    case "stop":
      return ["start"];
    case "paused":
      return ["resume", "stop"];
    case "idle":
      return ["resume", "pause", "stop"];
    case "run":
      return ["pause", "stop"];
  }
}
