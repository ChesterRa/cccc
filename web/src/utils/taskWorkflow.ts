export type TaskTypeId = "free" | "standard" | "optimization";

export type TaskTypeFamily = "free" | "standard" | "optimization";
export type TaskAttemptVerdict = "" | "keep" | "discard" | "crash" | "continue";

type TaskWorkflowChecklistItemLike = { text?: string | null; status?: string | null };

export type TaskWorkflowFields = {
  parentId?: string | null;
  parent_id?: string | null;
  taskType?: string | null;
  task_type?: string | null;
  status?: string | null;
  assignee?: string | null;
  outcome?: string | null;
  notes?: string | null;
  checklist?: string | TaskWorkflowChecklistItemLike[] | null;
};

export type TaskTypeDefinition = {
  id: TaskTypeId;
  family: TaskTypeFamily;
  label: string;
  description: string;
  requirements: string[];
  starterNotes: string;
  starterChecklist: string;
};

export type TaskWorkflowCoverage = {
  taskTypeId: TaskTypeId;
  taskTypeFamily: TaskTypeFamily;
  isRoot: boolean;
  isOptimization: boolean;
  missingSetup: string[];
  missingCloseout: string[];
  needsContract: boolean;
  needsCloseout: boolean;
  checklistTotal: number;
  checklistDone: number;
  hasGoal: boolean;
  hasSuccessCriteria: boolean;
  hasRequiredEvidence: boolean;
  hasOwner: boolean;
  hasOutcomeSummary: boolean;
  hasCloseoutVerdict: boolean;
  hasVerificationSummary: boolean;
  hasBaseline: boolean;
  hasPrimaryMetric: boolean;
  hasVerifierBoundary: boolean;
  hasAttemptDecision: boolean;
  hasCurrentBest: boolean;
  hasFrontierNext: boolean;
  hasAttemptLog: boolean;
  currentBestSummary: string;
  frontierNextSummary: string;
  latestAttemptVerdict: TaskAttemptVerdict;
  latestAttemptSummary: string;
  goalSummary: string;
};

const ROOT_SECTION_ORDER = [
  "Goal",
  "Success Criteria",
  "Required Evidence",
  "Next Control Move If Unmet",
  "Closeout Verdict",
  "Verification Summary",
  "Open Concerns",
] as const;

const OPTIMIZATION_CORE_SECTION_ORDER = [
  "Baseline",
  "Primary Metric",
  "Verifier Boundary",
  "Attempt Decision",
] as const;

const OPTIMIZATION_SECTION_ORDER = [
  ...OPTIMIZATION_CORE_SECTION_ORDER,
  "Current Best",
  "Frontier Next",
  "Attempt Log",
] as const;

const ALL_SECTION_HEADINGS = new Set(
  [...ROOT_SECTION_ORDER, ...OPTIMIZATION_SECTION_ORDER].map((item) => item.toLowerCase()),
);

const ROOT_NOTES_TEMPLATE = [
  "Goal:",
  "- [state the intended end result]",
  "",
  "Success Criteria:",
  "- [define the observable finish line]",
  "",
  "Required Evidence:",
  "- [name the proof, checks, or artifacts required]",
  "",
  "Next Control Move If Unmet:",
  "- [continue / request evidence / handoff / blocked / needs context]",
  "",
  "Closeout Verdict:",
  "- [DONE / DONE_WITH_CONCERNS / BLOCKED / NEEDS_CONTEXT]",
  "",
  "Verification Summary:",
  "- [summarize the proof gathered at closeout]",
  "",
  "Open Concerns:",
  "- [leave blank if none]",
].join("\n");

const OPTIMIZATION_NOTES_TEMPLATE = [
  ROOT_NOTES_TEMPLATE,
  "",
  "Baseline:",
  "- [capture the current measured state]",
  "",
  "Primary Metric:",
  "- [name the one metric that decides keep vs discard]",
  "",
  "Verifier Boundary:",
  "- [state what stays stable while testing]",
  "",
  "Attempt Decision:",
  "- [keep / discard / continue]",
  "",
  "Current Best:",
  "- [state the best kept branch or current leading result]",
  "",
  "Frontier Next:",
  "- [state the next bounded branch worth trying]",
  "",
  "Attempt Log:",
  "- [keep] [what changed] [metric before -> after] [evidence ref]",
].join("\n");

function checklistLines(items: string[]): string {
  return items.map((item) => `[ ] ${item}`).join("\n");
}

export const TASK_TYPES: TaskTypeDefinition[] = [
  {
    id: "free",
    family: "free",
    label: "Free",
    description:
      "Keep it lightweight. Use this when extra task structure would add more ceremony than control.",
    requirements: ["Keep the task lightweight unless more structure improves control."],
    starterNotes: "",
    starterChecklist: "",
  },
  {
    id: "standard",
    family: "standard",
    label: "Standard",
    description:
      "Use the normal closed-loop contract: goal, success criteria, required evidence, owner, and closeout.",
    requirements: [
      "Capture the goal, success criteria, and required evidence.",
      "Make the owner explicit.",
      "When closing, record the verdict and verification summary.",
    ],
    starterNotes: ROOT_NOTES_TEMPLATE,
    starterChecklist: checklistLines([
      "Confirm the goal and owner",
      "Define success criteria",
      "Name the required evidence",
      "Record the final verdict and verification summary",
    ]),
  },
  {
    id: "optimization",
    family: "optimization",
    label: "Optimization",
    description:
      "For metric-sensitive work. Capture baseline, primary metric, verifier boundary, current best, next frontier, and keep-or-discard closeout.",
    requirements: [
      "Capture the goal, success criteria, required evidence, and owner.",
      "Capture the baseline, primary metric, and verifier boundary.",
      "Keep the current best and next frontier explicit.",
      "At closeout, record keep or discard.",
    ],
    starterNotes: OPTIMIZATION_NOTES_TEMPLATE,
    starterChecklist: checklistLines([
      "Capture the baseline",
      "Name the primary metric",
      "Freeze the verifier boundary",
      "Run one bounded attempt",
      "Record keep or discard",
      "Update the current best and next frontier",
    ]),
  },
];

export function getTaskTypeDefinition(id: TaskTypeId): TaskTypeDefinition {
  return TASK_TYPES.find((item) => item.id === id) || TASK_TYPES[0];
}

function normalizeText(value: string | null | undefined): string {
  return String(value || "")
    .replace(/\r/g, "")
    .trim();
}

function stripPlaceholders(value: string): string {
  return value
    .replace(/\[[^\]]*\]/g, " ")
    .replace(/<[^>]*>/g, " ")
    .replace(/[•*_`]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function hasMeaningfulText(value: string | null | undefined): boolean {
  const normalized = normalizeText(value);
  if (!normalized) return false;
  return /[\p{L}\p{N}]/u.test(stripPlaceholders(normalized));
}

function normalizedParentId(fields: TaskWorkflowFields): string {
  return String(fields.parentId ?? fields.parent_id ?? "").trim();
}

function normalizeTaskTypeId(value: string | null | undefined): TaskTypeId | null {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (normalized === "free" || normalized === "optimization" || normalized === "standard")
    return normalized;
  if (normalized === "lean") return "free";
  if (
    normalized === "root" ||
    normalized === "planner" ||
    normalized === "reviewer" ||
    normalized === "debugger" ||
    normalized === "release"
  )
    return "standard";
  return null;
}

function explicitTaskType(fields: TaskWorkflowFields): TaskTypeId | null {
  return normalizeTaskTypeId(fields.taskType ?? fields.task_type);
}

function defaultTaskType(fields: TaskWorkflowFields): TaskTypeId {
  return normalizedParentId(fields) ? "free" : "standard";
}

function normalizeChecklistItemText(value: string | null | undefined): string {
  return String(value || "")
    .replace(/\[[^\]]*\]/g, " ")
    .replace(/^[-*]\s*/, "")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

function checklistItemsFromString(
  value: string | null | undefined,
): Array<{ text: string; status: string }> {
  return normalizeText(value)
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const match = line.match(/^\[([ xX~])\]\s*(.+)$/);
      const status = match
        ? match[1].toLowerCase() === "x"
          ? "done"
          : match[1] === "~"
            ? "in_progress"
            : "pending"
        : "pending";
      const text = (match ? match[2] : line).trim();
      return { text, status };
    })
    .filter((item) => normalizeChecklistItemText(item.text).length > 0);
}

function checklistItemsFromFields(
  checklist: string | TaskWorkflowChecklistItemLike[] | null | undefined,
): Array<{ text: string; status: string }> {
  if (typeof checklist === "string" || checklist == null) {
    return checklistItemsFromString(checklist);
  }
  if (!Array.isArray(checklist)) {
    return [];
  }
  return checklist
    .map((item) => ({
      text: String(item?.text || "").trim(),
      status:
        String(item?.status || "pending")
          .trim()
          .toLowerCase() || "pending",
    }))
    .filter((item) => normalizeChecklistItemText(item.text).length > 0);
}

function checklistProgress(fields: TaskWorkflowFields): { total: number; done: number } {
  const items = checklistItemsFromFields(fields.checklist);
  return { total: items.length, done: items.filter((item) => item.status === "done").length };
}

function normalizedStatus(fields: TaskWorkflowFields): string {
  return String(fields.status || "planned")
    .trim()
    .toLowerCase();
}

function isSectionHeading(line: string): boolean {
  const trimmed = String(line || "").trim();
  if (!trimmed.endsWith(":")) return false;
  return ALL_SECTION_HEADINGS.has(trimmed.slice(0, -1).trim().toLowerCase());
}

function extractSectionValue(notes: string | null | undefined, heading: string): string {
  const target = heading.trim().toLowerCase();
  const lines = normalizeText(notes).split("\n");
  const out: string[] = [];
  let capturing = false;
  for (const rawLine of lines) {
    const line = String(rawLine || "");
    const trimmed = line.trim();
    if (!capturing) {
      const match = trimmed.match(/^([^:]+):\s*(.*)$/);
      if (!match) continue;
      if (match[1].trim().toLowerCase() !== target) continue;
      capturing = true;
      if (match[2].trim()) out.push(match[2].trim());
      continue;
    }
    if (isSectionHeading(trimmed)) break;
    out.push(line);
  }
  return out.join("\n").trim();
}

function toInlineSummary(value: string | null | undefined): string {
  return String(value || "")
    .split("\n")
    .map((line) => line.trim().replace(/^[-*]\s*/, ""))
    .filter(Boolean)
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();
}

function normalizeAttemptVerdict(value: string | null | undefined): TaskAttemptVerdict {
  const normalized = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, "_");
  if (
    normalized === "keep" ||
    normalized === "discard" ||
    normalized === "crash" ||
    normalized === "continue"
  ) {
    return normalized;
  }
  return "";
}

function getLatestAttemptEntry(notes: string | null | undefined): {
  verdict: TaskAttemptVerdict;
  summary: string;
} {
  const section = extractSectionValue(notes, "Attempt Log");
  if (!hasMeaningfulText(section)) {
    return { verdict: "", summary: "" };
  }
  const lines = section
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const entries = lines
    .map((line) => line.replace(/^[-*]\s*/, "").trim())
    .map((line) => {
      const bracketMatch = line.match(/^\[([^\]]+)\]\s*(.+)$/);
      if (bracketMatch) {
        return {
          verdict: normalizeAttemptVerdict(bracketMatch[1]),
          summary: toInlineSummary(bracketMatch[2]),
        };
      }
      const inlineMatch = line.match(/^(keep|discard|crash|continue)\s*[:|-]\s*(.+)$/i);
      if (inlineMatch) {
        return {
          verdict: normalizeAttemptVerdict(inlineMatch[1]),
          summary: toInlineSummary(inlineMatch[2]),
        };
      }
      return { verdict: "" as TaskAttemptVerdict, summary: toInlineSummary(line) };
    })
    .filter((entry) => hasMeaningfulText(entry.summary));
  return entries.length > 0 ? entries[entries.length - 1] : { verdict: "", summary: "" };
}

export function getTaskGoalSummary(fields: TaskWorkflowFields): string {
  const goal = extractSectionValue(fields.notes, "Goal");
  if (hasMeaningfulText(goal)) return goal;
  return "";
}

export function getTaskDisplaySummary(fields: TaskWorkflowFields): string {
  const outcome = normalizeText(fields.outcome);
  if (hasMeaningfulText(outcome)) return outcome;
  const currentBest = extractSectionValue(fields.notes, "Current Best");
  if (hasMeaningfulText(currentBest)) return toInlineSummary(currentBest);
  return getTaskGoalSummary(fields);
}

export function recommendTaskType(fields: TaskWorkflowFields): TaskTypeId {
  return explicitTaskType(fields) || defaultTaskType(fields);
}

export function resolveTaskType(fields: TaskWorkflowFields): TaskTypeId {
  return explicitTaskType(fields) || defaultTaskType(fields);
}

export function evaluateTaskWorkflow(fields: TaskWorkflowFields): TaskWorkflowCoverage {
  const taskTypeId = resolveTaskType(fields);
  const taskTypeFamily = getTaskTypeDefinition(taskTypeId).family;
  const isRoot = !normalizedParentId(fields);
  const usesStandardWorkflow = taskTypeFamily === "standard";
  const isOptimization = taskTypeId === "optimization";
  const status = normalizedStatus(fields);
  const goalSummary = getTaskGoalSummary(fields);
  const hasGoal = hasMeaningfulText(goalSummary);
  const hasSuccessCriteria = hasMeaningfulText(
    extractSectionValue(fields.notes, "Success Criteria"),
  );
  const hasRequiredEvidence = hasMeaningfulText(
    extractSectionValue(fields.notes, "Required Evidence"),
  );
  const hasOwner = hasMeaningfulText(fields.assignee);
  const hasOutcomeSummary = hasMeaningfulText(fields.outcome);
  const hasCloseoutVerdict = hasMeaningfulText(
    extractSectionValue(fields.notes, "Closeout Verdict"),
  );
  const hasVerificationSummary = hasMeaningfulText(
    extractSectionValue(fields.notes, "Verification Summary"),
  );
  const hasBaseline = hasMeaningfulText(extractSectionValue(fields.notes, "Baseline"));
  const hasPrimaryMetric = hasMeaningfulText(extractSectionValue(fields.notes, "Primary Metric"));
  const hasVerifierBoundary = hasMeaningfulText(
    extractSectionValue(fields.notes, "Verifier Boundary"),
  );
  const hasAttemptDecision = hasMeaningfulText(
    extractSectionValue(fields.notes, "Attempt Decision"),
  );
  const currentBestSummary = toInlineSummary(extractSectionValue(fields.notes, "Current Best"));
  const frontierNextSummary = toInlineSummary(extractSectionValue(fields.notes, "Frontier Next"));
  const hasCurrentBest = hasMeaningfulText(currentBestSummary);
  const hasFrontierNext = hasMeaningfulText(frontierNextSummary);
  const latestAttempt = getLatestAttemptEntry(fields.notes);
  const hasAttemptLog = hasMeaningfulText(latestAttempt.summary);
  const checklistState = checklistProgress(fields);

  const missingSetup: string[] = [];
  if (usesStandardWorkflow || isOptimization) {
    if (!hasGoal) missingSetup.push("Goal");
    if (!hasSuccessCriteria) missingSetup.push("Success criteria");
    if (!hasRequiredEvidence) missingSetup.push("Required evidence");
  }
  if (usesStandardWorkflow && !hasOwner) {
    missingSetup.push("Owner");
  }
  if (isOptimization) {
    if (!hasBaseline) missingSetup.push("Baseline");
    if (!hasPrimaryMetric) missingSetup.push("Primary metric");
    if (!hasVerifierBoundary) missingSetup.push("Verifier boundary");
  }

  const missingCloseout: string[] = [];
  const expectsCloseout = status === "done" && (usesStandardWorkflow || isOptimization);
  if (expectsCloseout) {
    if (!hasOutcomeSummary) missingCloseout.push("Outcome summary");
    if (!hasCloseoutVerdict) missingCloseout.push("Closeout verdict");
    if (!hasVerificationSummary) missingCloseout.push("Verification summary");
    if (isOptimization && !hasAttemptDecision) missingCloseout.push("Attempt decision");
  }

  return {
    taskTypeId,
    taskTypeFamily,
    isRoot,
    isOptimization,
    missingSetup,
    missingCloseout,
    needsContract: missingSetup.length > 0,
    needsCloseout: missingCloseout.length > 0,
    checklistTotal: checklistState.total,
    checklistDone: checklistState.done,
    hasGoal,
    hasSuccessCriteria,
    hasRequiredEvidence,
    hasOwner,
    hasOutcomeSummary,
    hasCloseoutVerdict,
    hasVerificationSummary,
    hasBaseline,
    hasPrimaryMetric,
    hasVerifierBoundary,
    hasAttemptDecision,
    hasCurrentBest,
    hasFrontierNext,
    hasAttemptLog,
    currentBestSummary,
    frontierNextSummary,
    latestAttemptVerdict: latestAttempt.verdict,
    latestAttemptSummary: latestAttempt.summary,
    goalSummary,
  };
}

export function getTaskDoneTransitionBlockers(fields: TaskWorkflowFields): string[] {
  const coverage = evaluateTaskWorkflow({ ...fields, status: "done" });
  return coverage.needsCloseout ? coverage.missingCloseout : [];
}
