import type { CapabilityOverviewItem } from "../../../types";

function firstRecommendationLine(value?: string[]) {
  return Array.isArray(value) ? String(value[0] || "").trim() : "";
}

export function canManageSkillAssignments(row: Pick<CapabilityOverviewItem, "capability_id" | "kind">): boolean {
  const capabilityId = String(row.capability_id || "").trim();
  const kind = String(row.kind || "").trim().toLowerCase();
  return kind === "skill" && capabilityId.startsWith("skill:");
}

export function canManageSlashCommandVisibility(row: Pick<CapabilityOverviewItem, "capability_id" | "kind">): boolean {
  return canManageSkillAssignments(row);
}

export function canEditSkillRecord(row: Pick<CapabilityOverviewItem, "capability_id" | "kind" | "source_id">): boolean {
  return canManageSkillAssignments(row)
    && String(row.source_id || "").trim() === "agent_self_proposed";
}

export function isCapabilityHiddenFromSlashCommands(capabilityId: string, hiddenCapabilityIds: readonly string[]): boolean {
  const capId = String(capabilityId || "").trim();
  if (!capId) return false;
  return hiddenCapabilityIds.some((item) => String(item || "").trim() === capId);
}

export function nextSlashCommandHiddenCapabilities(
  hiddenCapabilityIds: readonly string[],
  capabilityId: string,
  visibleInSlashCommands: boolean,
): string[] {
  const capId = String(capabilityId || "").trim();
  const out: string[] = [];
  for (const item of hiddenCapabilityIds) {
    const value = String(item || "").trim();
    if (!value || value === capId || out.includes(value)) continue;
    out.push(value);
  }
  if (capId && !visibleInSlashCommands) out.push(capId);
  return out;
}

export function capabilityRegistryActionKind(row: Pick<CapabilityOverviewItem, "capability_id" | "kind" | "source_id">): "edit-skill" | "manage-skill-assignments" | "block" {
  if (canEditSkillRecord(row)) return "edit-skill";
  if (canManageSkillAssignments(row)) return "manage-skill-assignments";
  return "block";
}

export function capabilityRecommendationEntries(row: Pick<CapabilityOverviewItem, "use_when" | "evidence_kind" | "gotchas" | "avoid_when">): Array<{
  key: "use_when" | "verify_with" | "gotcha" | "avoid_when";
  value: string;
}> {
  return [
    { key: "use_when" as const, value: firstRecommendationLine(row.use_when) },
    { key: "verify_with" as const, value: String(row.evidence_kind || "").trim() },
    { key: "gotcha" as const, value: firstRecommendationLine(row.gotchas) },
    { key: "avoid_when" as const, value: firstRecommendationLine(row.avoid_when) },
  ].filter((entry) => entry.value);
}

export function capabilityOverviewLoadKey(input: {
  isActive: boolean;
  debouncedQuery: string;
  registryKind: string;
  registryPageSize: number;
  registryPolicy: string;
  registrySource: string;
  selfEvolvingSurface: boolean;
}): string {
  return JSON.stringify({
    isActive: Boolean(input.isActive),
    debouncedQuery: String(input.debouncedQuery || "").trim(),
    registryKind: String(input.registryKind || "all"),
    registryPageSize: Math.max(1, Math.trunc(Number(input.registryPageSize) || 1)),
    registryPolicy: String(input.registryPolicy || "all"),
    registrySource: String(input.registrySource || "all"),
    selfEvolvingSurface: Boolean(input.selfEvolvingSurface),
  });
}
