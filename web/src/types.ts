// Shared type definitions for CCCC Web UI

// Theme types
export type Theme = "light" | "dark" | "system";

export type GroupMeta = {
  group_id: string;
  title?: string;
  topic?: string;
  updated_at?: string;
  created_at?: string;
  running?: boolean;
  state?: "active" | "idle" | "paused";
};

export type GroupDoc = {
  group_id: string;
  title?: string;
  topic?: string;
  active_scope_key?: string;
  scopes?: Array<{ scope_key?: string; url?: string; label?: string }>;
  state?: "active" | "idle" | "paused";
};

// Event attachment metadata
export type EventAttachment = {
  kind?: string;
  path?: string;
  title?: string;
  bytes?: number;
  mime_type?: string;
};

// Chat message payload
export type ChatMessageData = {
  text?: string;
  to?: string[];
  priority?: "normal" | "attention";
  reply_required?: boolean;
  quote_text?: string;
  src_group_id?: string;
  src_event_id?: string;
  dst_group_id?: string;
  dst_to?: string[];
  attachments?: EventAttachment[];
};

export type ObligationStatus = {
  read: boolean;
  acked: boolean;
  replied: boolean;
  reply_required: boolean;
};

// Chat read receipt payload
export type ChatReadData = {
  actor_id?: string;
  event_id?: string;
};

// Ledger event data union
export type LedgerEventData = ChatMessageData | ChatReadData | Record<string, unknown>;

export type LedgerEvent = {
  id?: string;
  ts?: string;
  kind?: string;
  group_id?: string;
  by?: string;
  data?: LedgerEventData;
  _read_status?: Record<string, boolean>;
  _ack_status?: Record<string, boolean>;
  _obligation_status?: Record<string, ObligationStatus>;
};

export type Actor = {
  id: string;
  role?: string;
  title?: string;
  enabled?: boolean;
  running?: boolean;  // Actual process running status
  command?: string[];
  env?: Record<string, string>;
  runner?: string;
  runtime?: string;
  submit?: "enter" | "newline" | "none";
  profile_id?: string;
  profile_revision_applied?: number;
  updated_at?: string;
  unread_count?: number;
};

export type ActorProfile = {
  id: string;
  name: string;
  runtime: SupportedRuntime | string;
  runner: "pty" | "headless";
  command: string[];
  submit: "enter" | "newline" | "none";
  env: Record<string, string>;
  created_at: string;
  updated_at: string;
  revision: number;
  usage_count?: number;
};

export type ActorProfileUsage = {
  group_id: string;
  group_title?: string;
  actor_id: string;
  actor_title?: string;
};

export type PresenceAgent = {
  id: string;
  status?: string | null;
  updated_at?: string | null;
};

export type RuntimeInfo = {
  name: string;
  display_name: string;
  command: string;
  recommended_command?: string;
  available: boolean;
  path?: string;
  capabilities: string;
};

export type ReplyTarget = {
  eventId: string;
  by: string;
  text: string;
} | null;

export type TaskStep = {
  id: string;
  name: string;
  acceptance?: string | null;
  status?: string | null;
};

export type Task = {
  id: string;
  name: string;
  goal?: string | null;
  status?: string | null;
  milestone?: string | null;
  assignee?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  steps?: TaskStep[];
  current_step?: string | null;
  progress?: number | null;
};

export type GroupContext = {
  version?: string;
  vision?: string | null;
  sketch?: string | null;
  milestones?: Array<{
    id: string;
    name: string;
    description?: string | null;
    status?: string | null;
    started?: string | null;
    completed?: string | null;
    outcomes?: string | null;
  }>;
  notes?: Array<{
    id: string;
    content: string;
  }>;
  references?: Array<{
    id: string;
    url: string;
    note?: string | null;
  }>;
  tasks_summary?: {
    total: number;
    done: number;
    active: number;
    planned: number;
  };
  active_task?: {
    id: string;
    name: string;
    goal?: string | null;
    status?: string | null;
    milestone?: string | null;
    assignee?: string | null;
    created_at?: string | null;
    updated_at?: string | null;
    steps?: Array<{ id: string; name: string; acceptance?: string | null; status?: string | null }>;
    current_step?: string | null;
    progress?: number | null;
  } | null;
  presence?: {
    agents?: PresenceAgent[];
  };
};

export type ProjectMdInfo = {
  found: boolean;
  path?: string | null;
  content?: string | null;
  error?: string | null;
};

export type GroupSettings = {
  default_send_to: "foreman" | "broadcast";
  nudge_after_seconds: number;
  reply_required_nudge_after_seconds: number;
  attention_ack_nudge_after_seconds: number;
  unread_nudge_after_seconds: number;
  nudge_digest_min_interval_seconds: number;
  nudge_max_repeats_per_obligation: number;
  nudge_escalate_after_repeats: number;
  actor_idle_timeout_seconds: number;
  keepalive_delay_seconds: number;
  keepalive_max_per_actor: number;
  silence_timeout_seconds: number;
  help_nudge_interval_seconds: number;
  help_nudge_min_messages: number;
  min_interval_seconds: number;
  auto_mark_on_delivery: boolean;

  terminal_transcript_visibility: "off" | "foreman" | "all";
  terminal_transcript_notify_tail: boolean;
  terminal_transcript_notify_lines: number;
};

export type RemoteAccessState = {
  provider: "off" | "manual" | "tailscale" | string;
  mode: string;
  enforce_web_token: boolean;
  enabled: boolean;
  status: "stopped" | "running" | "not_installed" | "not_authenticated" | "misconfigured" | "error" | string;
  endpoint?: string | null;
  updated_at?: string | null;
  diagnostics?: {
    web_token_present?: boolean;
    web_host?: string;
    web_port?: number;
    web_public_url?: string | null;
    web_bind_loopback?: boolean;
    web_bind_reachable?: boolean;
    mode_supported?: boolean;
    tailscale_installed?: boolean | null;
    tailscale_backend_state?: string | null;
  } | null;
  next_steps?: string[] | null;
};

export type AutomationRuleTriggerInterval = {
  kind: "interval";
  every_seconds: number;
};

export type AutomationRuleTriggerCron = {
  kind: "cron";
  cron: string;
  timezone?: string;
};

export type AutomationRuleTriggerAt = {
  kind: "at";
  at: string;
};

export type AutomationRuleTrigger =
  | AutomationRuleTriggerInterval
  | AutomationRuleTriggerCron
  | AutomationRuleTriggerAt;

export type AutomationRuleAction = {
  kind: "notify";
  title?: string;
  snippet_ref?: string | null;
  message?: string;
  priority?: "low" | "normal" | "high" | "urgent";
  requires_ack?: boolean;
} | {
  kind: "group_state";
  state: "active" | "idle" | "paused" | "stopped";
} | {
  kind: "actor_control";
  operation: "start" | "stop" | "restart";
  targets?: string[];
};

export type AutomationRule = {
  id: string;
  enabled?: boolean;
  scope?: "group" | "personal";
  owner_actor_id?: string | null;
  to?: string[];
  trigger?: AutomationRuleTrigger;
  action?: AutomationRuleAction;
};

export type AutomationRuleSet = {
  rules: AutomationRule[];
  snippets: Record<string, string>;
};

export type AutomationRuleStatus = {
  last_fired_at?: string;
  last_error_at?: string;
  last_error?: string;
  next_fire_at?: string;
  completed?: boolean;
  completed_at?: string;
};

export type GroupAutomation = {
  ruleset: AutomationRuleSet;
  status: Record<string, AutomationRuleStatus>;
  config_path: string;
  supported_vars: string[];
  version?: number;
  server_now?: string;
};

export type IMPlatform = "telegram" | "slack" | "discord" | "feishu" | "dingtalk";

export type IMConfig = {
  platform?: IMPlatform;
  // Legacy single token field (backward compat)
  token_env?: string;
  token?: string;
  // Dual token fields for Slack (bot_token for outbound, app_token for inbound)
  bot_token_env?: string;
  app_token_env?: string;
  // Feishu fields
  feishu_domain?: string;
  feishu_app_id?: string;
  feishu_app_id_env?: string;
  feishu_app_secret?: string;
  feishu_app_secret_env?: string;
  // DingTalk fields
  dingtalk_app_key?: string;
  dingtalk_app_key_env?: string;
  dingtalk_app_secret?: string;
  dingtalk_app_secret_env?: string;
  dingtalk_robot_code?: string;
  dingtalk_robot_code_env?: string;
};

export type IMStatus = {
  group_id: string;
  configured: boolean;
  platform?: string;
  running: boolean;
  pid?: number;
  subscribers: number;
};

export type DirItem = { name: string; path: string; is_dir: boolean };
export type DirSuggestion = { name: string; path: string; icon: string };

// Runtime configuration
export const SUPPORTED_RUNTIMES = [
  "claude",
  "codex",
  "droid",
  "amp",
  "auggie",
  "neovate",
  "gemini",
  "cursor",
  "kilocode",
  "opencode",
  "copilot",
  "custom",
] as const;

export type SupportedRuntime = typeof SUPPORTED_RUNTIMES[number];

export const RUNTIME_INFO: Record<string, { label: string; desc: string }> = {
  amp: { label: "Amp", desc: "" },
  auggie: { label: "Auggie (Augment)", desc: "" },
  claude: { label: "Claude Code", desc: "" },
  codex: { label: "Codex CLI", desc: "" },
  cursor: { label: "Cursor CLI", desc: "Manual MCP installation needed" },
  droid: { label: "Droid", desc: "" },
  gemini: { label: "Gemini CLI", desc: "" },
  kilocode: { label: "Kilo Code", desc: "Manual MCP installation needed" },
  neovate: { label: "Neovate Code", desc: "" },
  opencode: { label: "OpenCode", desc: "Manual MCP installation needed" },
  copilot: { label: "GitHub Copilot", desc: "Manual MCP installation needed" },
  custom: { label: "Custom", desc: "Manual MCP installation needed" },
};

// Runtime colors for visual distinction
// Dark theme: semi-transparent dark backgrounds with bright text
// Light theme: semi-transparent light backgrounds with darker text
export const RUNTIME_COLORS: Record<string, { 
  // Dark theme
  bg: string; 
  text: string; 
  border: string; 
  dot: string;
  // Light theme
  bgLight: string;
  textLight: string;
  borderLight: string;
  dotLight: string;
}> = {
  amp: {
    bg: "bg-rose-900/30", text: "text-rose-300", border: "border-rose-600/50", dot: "bg-rose-400",
    bgLight: "bg-rose-50", textLight: "text-rose-700", borderLight: "border-rose-300", dotLight: "bg-rose-500"
  },
  auggie: {
    bg: "bg-teal-900/30", text: "text-teal-300", border: "border-teal-600/50", dot: "bg-teal-400",
    bgLight: "bg-teal-50", textLight: "text-teal-700", borderLight: "border-teal-300", dotLight: "bg-teal-500"
  },
  claude: { 
    bg: "bg-orange-900/30", text: "text-orange-300", border: "border-orange-600/50", dot: "bg-orange-400",
    bgLight: "bg-orange-50", textLight: "text-orange-700", borderLight: "border-orange-300", dotLight: "bg-orange-500"
  },
  codex: { 
    bg: "bg-emerald-900/30", text: "text-emerald-300", border: "border-emerald-600/50", dot: "bg-emerald-400",
    bgLight: "bg-emerald-50", textLight: "text-emerald-700", borderLight: "border-emerald-300", dotLight: "bg-emerald-500"
  },
  cursor: {
    bg: "bg-indigo-900/30", text: "text-indigo-300", border: "border-indigo-600/50", dot: "bg-indigo-400",
    bgLight: "bg-indigo-50", textLight: "text-indigo-700", borderLight: "border-indigo-300", dotLight: "bg-indigo-500"
  },
  droid: { 
    bg: "bg-violet-900/30", text: "text-violet-300", border: "border-violet-600/50", dot: "bg-violet-400",
    bgLight: "bg-violet-50", textLight: "text-violet-700", borderLight: "border-violet-300", dotLight: "bg-violet-500"
  },
  gemini: {
    bg: "bg-yellow-900/30", text: "text-yellow-300", border: "border-yellow-600/50", dot: "bg-yellow-400",
    bgLight: "bg-yellow-50", textLight: "text-yellow-700", borderLight: "border-yellow-300", dotLight: "bg-yellow-500"
  },
  kilocode: {
    bg: "bg-pink-900/30", text: "text-pink-300", border: "border-pink-600/50", dot: "bg-pink-400",
    bgLight: "bg-pink-50", textLight: "text-pink-700", borderLight: "border-pink-300", dotLight: "bg-pink-500"
  },
  neovate: {
    bg: "bg-fuchsia-900/30", text: "text-fuchsia-300", border: "border-fuchsia-600/50", dot: "bg-fuchsia-400",
    bgLight: "bg-fuchsia-50", textLight: "text-fuchsia-700", borderLight: "border-fuchsia-300", dotLight: "bg-fuchsia-500"
  },
  opencode: { 
    bg: "bg-cyan-900/30", text: "text-cyan-300", border: "border-cyan-600/50", dot: "bg-cyan-400",
    bgLight: "bg-cyan-50", textLight: "text-cyan-700", borderLight: "border-cyan-300", dotLight: "bg-cyan-500"
  },
  copilot: { 
    bg: "bg-slate-800/50", text: "text-slate-300", border: "border-slate-500/50", dot: "bg-slate-400",
    bgLight: "bg-gray-100", textLight: "text-gray-700", borderLight: "border-gray-300", dotLight: "bg-gray-500"
  },
  custom: {
    bg: "bg-zinc-800/50", text: "text-zinc-300", border: "border-zinc-500/50", dot: "bg-zinc-400",
    bgLight: "bg-zinc-100", textLight: "text-zinc-700", borderLight: "border-zinc-300", dotLight: "bg-zinc-500"
  },
  user: { 
    bg: "bg-sky-900/30", text: "text-sky-300", border: "border-sky-600/50", dot: "bg-sky-400",
    bgLight: "bg-sky-50", textLight: "text-sky-700", borderLight: "border-sky-300", dotLight: "bg-sky-500"
  },
};

// Helper to get runtime color, with fallback
export function getRuntimeColor(runtime?: string, isDark: boolean = true) {
  const colors = RUNTIME_COLORS[runtime || "codex"] || RUNTIME_COLORS.codex;
  if (isDark) {
    return {
      bg: colors.bg,
      text: colors.text,
      border: colors.border,
      dot: colors.dot,
    };
  }
  return {
    bg: colors.bgLight,
    text: colors.textLight,
    border: colors.borderLight,
    dot: colors.dotLight,
  };
}

const ACTOR_ACCENTS = [
  // Dark theme accents are intentionally soft (low-saturation ring + readable name color).
  { ring: "ring-sky-400/35", text: "text-sky-300", ringLight: "ring-sky-300", textLight: "text-sky-700" },
  { ring: "ring-indigo-400/35", text: "text-indigo-300", ringLight: "ring-indigo-300", textLight: "text-indigo-700" },
  { ring: "ring-violet-400/35", text: "text-violet-300", ringLight: "ring-violet-300", textLight: "text-violet-700" },
  { ring: "ring-fuchsia-400/35", text: "text-fuchsia-300", ringLight: "ring-fuchsia-300", textLight: "text-fuchsia-700" },
  { ring: "ring-cyan-400/35", text: "text-cyan-300", ringLight: "ring-cyan-300", textLight: "text-cyan-700" },
  { ring: "ring-teal-400/35", text: "text-teal-300", ringLight: "ring-teal-300", textLight: "text-teal-700" },
  { ring: "ring-emerald-400/35", text: "text-emerald-300", ringLight: "ring-emerald-300", textLight: "text-emerald-700" },
  { ring: "ring-amber-400/35", text: "text-amber-300", ringLight: "ring-amber-300", textLight: "text-amber-700" },
];

function _fnv1a32(input: string): number {
  // Deterministic, fast, and stable across JS engines.
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

export function getActorAccentColor(actorId?: string, isDark: boolean = true) {
  const id = String(actorId || "").trim();
  const idx = id ? _fnv1a32(id) % ACTOR_ACCENTS.length : 0;
  const a = ACTOR_ACCENTS[idx] || ACTOR_ACCENTS[0];
  if (isDark) return { ring: a.ring, text: a.text };
  return { ring: a.ringLight, text: a.textLight };
}
