import type {
  CapabilityDomain,
  CapabilityMemberProjection,
  CapabilityOverviewItem,
  CapabilitySupport,
  CapabilityVerification,
} from "../../types";
import { capabilityCenterDisplayName, capabilityCenterType } from "./capabilityCenterModel";

/** The catalog source the daemon gives the recommendations shipped with this version. */
export const RECOMMENDATION_SOURCE_ID = "cccc_recommended";

/** One summary of a capability's state in this Group; the first matching rule wins. */
export type RecommendationDot =
  | "blocked"
  | "group"
  | "member"
  | "available"
  | "setup"
  | "pending"
  | "unknown"
  | "unsupported"
  | "none";

/** Legend order: how far it is from being used, then policy. The dashed ring stands for both
 * "pending" and "unknown", so the legend gives it one wording and the hover tip the reason. */
export const RECOMMENDATION_LEGEND: readonly Exclude<RecommendationDot, "none">[] = [
  "group",
  "member",
  "available",
  "setup",
  "unknown",
  "unsupported",
  "blocked",
];

export type RecommendationBinding = "group" | "member" | "none";

type DotInput = Pick<
  CapabilityOverviewItem,
  "blocked_global" | "blocked_group" | "group_enabled" | "members"
>;

export function isReleaseRecommendation(
  item: Pick<CapabilityOverviewItem, "content_origin"> | null | undefined,
): boolean {
  return item?.content_origin === "release_recommendation";
}

function usable(member: CapabilityMemberProjection): boolean {
  return member.availability === "compatible" && !member.blocked && !member.activation_pending;
}

/** Enabled with a token that has not been checked yet with the current recipe. */
function pending(member: CapabilityMemberProjection): boolean {
  return (
    member.availability === "compatible" && !member.blocked && Boolean(member.activation_pending)
  );
}

function memberScoped(member: CapabilityMemberProjection): boolean {
  return (member.enabled_scopes || []).some((scope) => scope === "actor" || scope === "session");
}

/**
 * Summarizes the daemon's per-member projection. The daemon decides each member's
 * availability; this only picks the first rule that holds.
 */
export function recommendationDot(item: DotInput): RecommendationDot {
  if (item.blocked_global || item.blocked_group) return "blocked";
  const members = item.members || [];
  if (members.length === 0) return "none";
  const ready = members.filter(usable);
  if (item.group_enabled && ready.length > 0) return "group";
  if (ready.some(memberScoped)) return "member";
  // Someone enabled it with a token not checked yet, so "nobody enabled it" would be wrong.
  if (members.some(pending)) return "pending";
  if (ready.length > 0) return "available";
  if (members.some((member) => member.availability === "needs_setup")) return "setup";
  if (members.some((member) => member.availability === "unknown")) return "unknown";
  return "unsupported";
}

/** The same rules for one member's own line. */
export function recommendationMemberDot(
  item: Pick<CapabilityOverviewItem, "group_enabled">,
  member: CapabilityMemberProjection,
): RecommendationDot {
  if (member.blocked) return "blocked";
  return recommendationDot({ group_enabled: item.group_enabled, members: [member] });
}

/** Which binding exists, whether or not anyone can use it now. */
export function recommendationBinding(
  item: Pick<CapabilityOverviewItem, "group_enabled" | "members">,
): RecommendationBinding {
  if (item.group_enabled) return "group";
  if ((item.members || []).some(memberScoped)) return "member";
  return "none";
}

/** Enabling for the Group is offered when some member can use it, or might. */
export function recommendationCanEnableForGroup(item: DotInput): boolean {
  const dot = recommendationDot(item);
  return dot === "member" || dot === "available" || dot === "pending" || dot === "unknown";
}

const LEVEL_RANK: Record<CapabilityVerification["level"], number> = {
  listed: 1,
  install_checked: 2,
  tool_call_checked: 3,
  task_checked: 4,
};

/** The highest level observed decides; at one level the latest observation wins. */
export function recommendationVerification(
  observations: CapabilityVerification[] | undefined,
): CapabilityVerification | null {
  let top: CapabilityVerification | null = null;
  for (const item of observations || []) {
    if (!item || !(item.level in LEVEL_RANK)) continue;
    if (item.outcome !== "passed" && item.outcome !== "failed") continue;
    if (
      !top ||
      LEVEL_RANK[item.level] > LEVEL_RANK[top.level] ||
      (LEVEL_RANK[item.level] === LEVEL_RANK[top.level] &&
        String(item.observed_at || "") > String(top.observed_at || ""))
    ) {
      top = item;
    }
  }
  return top;
}

/**
 * The i18n key of the verification label: passed and failed read differently. A text skill's
 * install check only compares its imported text with the original, so it is not called installed:
 * that would read as if the tool the skill drives were installed.
 */
export function recommendationVerificationKey(
  item: CapabilityVerification | null,
  form?: string,
): string {
  if (!item) return "none";
  const level =
    item.level === "install_checked" && form === "text_skill" ? "text_checked" : item.level;
  return `${item.outcome === "failed" ? "failed" : "passed"}.${level}`;
}

export type RecommendationCell = {
  primary: CapabilityOverviewItem | null;
  alternatives: CapabilityOverviewItem[];
};

export type RecommendationRow = {
  domain: CapabilityDomain;
  mcp: RecommendationCell;
  skill: RecommendationCell;
};

function byName(left: CapabilityOverviewItem, right: CapabilityOverviewItem): number {
  return capabilityCenterDisplayName(left).localeCompare(capabilityCenterDisplayName(right));
}

/** Rows in the release's domain order; a cell holds its primary, then the alternatives. */
export function recommendationRows(
  items: CapabilityOverviewItem[],
  domains: CapabilityDomain[],
): RecommendationRow[] {
  return domains.map((domain) => {
    const found = {
      mcp: {
        primaries: [] as CapabilityOverviewItem[],
        alternatives: [] as CapabilityOverviewItem[],
      },
      skill: {
        primaries: [] as CapabilityOverviewItem[],
        alternatives: [] as CapabilityOverviewItem[],
      },
    };
    for (const item of items) {
      const entry = (item.recommendation || []).find(
        (value) => value.domain_id === domain.domain_id,
      );
      if (!entry) continue;
      const cell = found[capabilityCenterType(item) === "skill" ? "skill" : "mcp"];
      (entry.role === "primary" ? cell.primaries : cell.alternatives).push(item);
    }
    const cell = (kind: "mcp" | "skill"): RecommendationCell => {
      // The release lists one primary per cell; a second one is shown first among the others.
      const primaries = [...found[kind].primaries].sort(byName);
      return {
        primary: primaries[0] || null,
        alternatives: [...primaries.slice(1), ...[...found[kind].alternatives].sort(byName)],
      };
    };
    return { domain, mcp: cell("mcp"), skill: cell("skill") };
  });
}

export function recommendationEntry(
  item: Pick<CapabilityOverviewItem, "recommendation">,
  domainId: string,
) {
  return (item.recommendation || []).find((value) => value.domain_id === domainId) || null;
}

/** The first row's first primary, so a wide screen opens on a real example. */
export function firstRecommendation(rows: RecommendationRow[]): string {
  for (const row of rows) {
    const item = row.mcp.primary || row.skill.primary;
    if (item) return String(item.capability_id || "");
  }
  return "";
}

export function recommendationCount(rows: RecommendationRow[]): number {
  const ids = new Set<string>();
  for (const row of rows) {
    for (const cell of [row.mcp, row.skill]) {
      for (const item of [cell.primary, ...cell.alternatives]) {
        if (item) ids.add(String(item.capability_id || ""));
      }
    }
  }
  return ids.size;
}

/** i18n keys for where it runs and what it needs; unknown values show nothing. */
export function recommendationRunsOnKey(support: CapabilitySupport | undefined): string {
  const value = String(support?.runs_on || "");
  return ["daemon_host", "actor_tools", "reference"].includes(value) ? value : "";
}

export function recommendationAuthKey(support: CapabilitySupport | undefined): string {
  const value = String(support?.auth || "");
  return ["none", "bearer", "api_key", "oauth", "account"].includes(value) ? value : "";
}

/** The reason codes the daemon gives; others are shown as they come. */
export const RECOMMENDATION_REASON_CODES: ReadonlySet<string> = new Set([
  "actor_tool_prerequisites_unknown",
  "blocked_by_global_policy",
  "blocked_by_group_policy",
  "credential_permission_denied",
  "credential_rejected",
  "credential_route_not_supported",
  "delivery_form_not_supported",
  "delivery_form_unknown",
  "external_mcp_not_supported_by_actor",
  "missing_daemon_environment",
  "missing_daemon_executable",
  "missing_member_credential",
  "unverified_remote_search",
]);

/** A local calendar date, as the verification list shows it. */
export function recommendationDate(isoStr: string | undefined): string {
  const date = new Date(String(isoStr || ""));
  if (isNaN(date.getTime())) return "";
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** A link's text without the scheme, so the address reads as written. */
export function recommendationSourceLabel(uri: string | undefined): string {
  return String(uri || "")
    .trim()
    .replace(/^https?:\/\//i, "")
    .replace(/\/$/, "");
}
