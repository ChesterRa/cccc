import { parsePrivateEnvSetText } from "../../utils/privateEnvInput";

export interface ActorAdvancedDraft {
  /** True when the launch command no longer follows the runtime default. */
  commandOverridden: boolean;
  /** Raw environment variable draft, as typed. */
  environmentText: string;
  capabilityIds: readonly string[];
}

export type ActorAdvancedChange =
  | { kind: "command" }
  | { kind: "environment"; count: number }
  | { kind: "capabilities"; count: number };

/**
 * What a closed Advanced section must still report, so a setting changed inside it is never
 * forgotten once the section is collapsed.
 */
export function actorAdvancedChanges(draft: ActorAdvancedDraft): ActorAdvancedChange[] {
  const changes: ActorAdvancedChange[] = [];
  if (draft.commandOverridden) changes.push({ kind: "command" });
  const parsed = parsePrivateEnvSetText(draft.environmentText);
  // A draft that does not parse yet is still something the user typed and must stay visible.
  const environmentCount = parsed.ok
    ? Object.keys(parsed.setVars).length
    : draft.environmentText.trim()
      ? 1
      : 0;
  if (environmentCount > 0) changes.push({ kind: "environment", count: environmentCount });
  const capabilityCount = new Set(draft.capabilityIds.map((id) => id.trim()).filter(Boolean)).size;
  if (capabilityCount > 0) changes.push({ kind: "capabilities", count: capabilityCount });
  return changes;
}
