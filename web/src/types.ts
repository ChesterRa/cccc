// Shared type definitions for CCCC Web UI

// Theme types
export type Theme = "light" | "dark" | "system";
export type TextScale = 90 | 100 | 125;

export type GroupRuntimeStatus = {
  lifecycle_state: "active" | "idle" | "paused" | "stopped" | string;
  runtime_running: boolean;
  running_actor_count: number;
  has_running_foreman: boolean;
};

export type GroupMeta = {
  group_id: string;
  title?: string;
  topic?: string;
  updated_at?: string;
  created_at?: string;
  running?: boolean;
  state?: "active" | "idle" | "paused" | "stopped";
  runtime_status?: GroupRuntimeStatus;
};

export type GroupDoc = {
  group_id: string;
  title?: string;
  topic?: string;
  active_scope_key?: string;
  scopes?: Array<{ scope_key?: string; url?: string; label?: string }>;
  running?: boolean;
  state?: "active" | "idle" | "paused" | "stopped";
  runtime_status?: GroupRuntimeStatus;
};

// Server-backed attachment metadata carried by canonical ledger events.
export type EventAttachment = {
  kind?: string;
  path?: string;
  title?: string;
  bytes?: number;
  mime_type?: string;
  local_preview_url?: string;
};

// Local-only preview attachment used by optimistic messages before upload confirmation.
export type OptimisticAttachment = {
  kind?: string;
  path?: string;
  title?: string;
  bytes?: number;
  mime_type?: string;
  local_preview_url: string;
};

export type MessageAttachment = EventAttachment | OptimisticAttachment;

export type MessageRef = {
  kind?: string;
  [key: string]: unknown;
};

export type PresentationRefStatus = "open" | "needs_user" | "resolved";

export type PresentationRefSnapshot = {
  path: string;
  mime_type?: string;
  bytes?: number;
  sha256?: string;
  width?: number;
  height?: number;
  captured_at?: string;
  source?: string;
};

export type PresentationMessageRef = MessageRef & {
  kind: "presentation_ref";
  v?: number;
  slot_id: string;
  label?: string;
  locator_label?: string;
  title?: string;
  card_type?: string;
  status?: PresentationRefStatus | string;
  href?: string;
  excerpt?: string;
  locator?: Record<string, unknown>;
  snapshot?: PresentationRefSnapshot;
};

export type TaskMessageRef = MessageRef & {
  kind: "task_ref";
  task_id: string;
  title?: string;
  status?: string;
  waiting_on?: TaskWaitingOn | null;
  handoff_to?: string | null;
};

export type StreamingActivity = {
  id: string;
  kind: "queued" | "thinking" | "plan" | "search" | "command" | "patch" | "tool" | "reply" | string;
  status: "started" | "updated" | "completed" | string;
  summary: string;
  detail?: string;
  ts?: string;
  raw_item_type?: string;
  tool_name?: string;
  server_name?: string;
  command?: string;
  cwd?: string;
  file_paths?: string[];
  query?: string;
};

export type HeadlessPreviewBlock = {
  id: string;
  streamId: string;
  streamPhase: string;
  text: string;
  updatedAt: string;
  completed: boolean;
  transient: boolean;
};

export type HeadlessPreviewSession = {
  actorId: string;
  pendingEventId: string;
  currentStreamId: string;
  phase: string;
  streamPhase: string;
  updatedAt: string;
  latestText: string;
  transcriptBlocks: HeadlessPreviewBlock[];
  activities: StreamingActivity[];
};

// Chat message payload
export type ChatMessageData = {
  text?: string;
  to?: string[];
  priority?: "normal" | "attention";
  reply_required?: boolean;
  sender_title?: string;
  sender_runtime?: string;
  sender_avatar_path?: string;
  source_platform?: string;
  reply_to?: string;
  stream_id?: string;
  stream_phase?: string;
  pending_event_id?: string;
  pending_placeholder?: boolean;
  client_id?: string;
  quote_text?: string;
  src_group_id?: string;
  src_event_id?: string;
  dst_group_id?: string;
  dst_to?: string[];
  activities?: StreamingActivity[];
  refs?: MessageRef[];
  attachments?: MessageAttachment[];
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
  _streaming?: boolean;
  _read_status?: Record<string, boolean>;
  _ack_status?: Record<string, boolean>;
  _obligation_status?: Record<string, ObligationStatus>;
};

export type HeadlessStreamEvent = {
  id?: string;
  ts?: string;
  group_id?: string;
  actor_id?: string;
  type?: string;
  data?: Record<string, unknown>;
};

export type LedgerEventStatusPayload = {
  read_status?: Record<string, boolean>;
  ack_status?: Record<string, boolean>;
  obligation_status?: Record<string, ObligationStatus>;
};

export type Actor = {
  id: string;
  role?: string;
  internal_kind?: string | null;
  title?: string;
  avatar_url?: string | null;
  has_custom_avatar?: boolean;
  enabled?: boolean;
  running?: boolean;  // Actual process running status
  idle_seconds?: number | null;  // Seconds since last PTY output (null if not running/headless)
  effective_working_state?: "stopped" | "idle" | "working" | "waiting" | "stuck" | string;
  effective_working_reason?: string;
  effective_working_updated_at?: string | null;
  effective_active_task_id?: string | null;
  command?: string[];
  env?: Record<string, string>;
  capability_autoload?: string[];
  runner?: string;
  runner_effective?: string;
  runtime?: string;
  submit?: "enter" | "newline" | "none";
  profile_id?: string;
  profile_scope?: "global" | "user";
  profile_owner?: string;
  profile_revision_applied?: number;
  updated_at?: string;
  unread_count?: number;
  web_model_queued_count?: number;
  web_model_queued_after_event_id?: string;
  web_model_queued_latest_event_id?: string;
  web_model_queued_latest_ts?: string;
};

export type ActorProfile = {
  id: string;
  name: string;
  scope?: "global" | "user";
  owner_id?: string;
  runtime: SupportedRuntime | string;
  runner: "pty" | "headless";
  command: string[];
  submit: "enter" | "newline" | "none";
  env: Record<string, string>;
  capability_defaults?: {
    autoload_capabilities?: string[];
    default_scope?: "actor" | "session";
    session_ttl_seconds?: number;
  };
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

export type CapabilityRecentSuccess = {
  success_count: number;
  last_success_at?: string;
  last_group_id?: string;
  last_actor_id?: string;
  last_action?: string;
};

export type CapabilityReadinessPreview = {
  preview_status?: string;
  next_step?: string;
  preview_basis?: string[];
  policy_level?: string;
  enable_supported?: boolean;
  already_active?: boolean;
  install_mode?: string;
  required_env?: string[];
  missing_env?: string[];
  cached_install_state?: string;
  install_error_code?: string;
  enable_block_reason?: string;
  policy_source?: string;
  policy_mode?: string;
};

export type CapabilityOverviewItem = {
  capability_id: string;
  kind?: string;
  name?: string;
  description_short?: string;
  use_when?: string[];
  avoid_when?: string[];
  gotchas?: string[];
  evidence_kind?: string;
  source_id?: string;
  source_uri?: string;
  source_record_id?: string;
  source_record_version?: string;
  origin_group_id?: string;
  source_tier?: string;
  trust_tier?: string;
  license?: string;
  sync_state?: string;
  updated_at_source?: string;
  last_synced_at?: string;
  policy_level?: string;
  policy_visible?: boolean;
  enable_supported?: boolean;
  qualification_status?: string;
  qualification_reasons?: string[];
  install_mode?: string;
  tags?: string[];
  blocked_global?: boolean;
  blocked_reason?: string;
  autoload_candidate?: boolean;
  recent_success?: CapabilityRecentSuccess;
  readiness_preview?: CapabilityReadinessPreview;
  cached_install_state?: string;
  cached_install_error_code?: string;
  cached_install_error?: string;
  capsule_text?: string;
  tool_count?: number;
  tool_names?: string[];
};

export type CapabilitySourceState = {
  source_id: string;
  enabled: boolean;
  source_level?: string;
  rationale?: string;
  sync_state?: string;
  last_synced_at?: string;
  staleness_seconds?: number;
  record_count?: number;
  error?: string;
};

export type CapabilityBlockEntry = {
  capability_id: string;
  scope?: string;
  reason?: string;
  by?: string;
  blocked_at?: string;
  expires_at?: string;
};

export type CapabilityEnabledEntry = {
  capability_id: string;
  scope?: string;
  actor_id?: string;
  enabled_at?: string;
  expires_at?: string;
  ttl_seconds?: number;
  reason?: string;
  tool_count?: number;
  tool_names?: string[];
};

export type CapabilityStateResult = {
  group_id: string;
  actor_id: string;
  enabled: CapabilityEnabledEntry[];
  enabled_capabilities?: string[];
  dynamic_tools?: Array<{ name: string; capability_id: string; description?: string }>;
  active_capsule_skills?: Array<{
    capability_id: string;
    name?: string;
    description_short?: string;
    capsule_preview?: string;
    capsule_text?: string;
    source_id?: string;
    source_uri?: string;
    policy_level?: string;
  }>;
  capability_usage?: CapabilityUsageSummary;
};

export type CapabilityUsageActorEntry = {
  actor_id: string;
  actor_title?: string;
  label?: string;
  expires_at?: string;
  ttl_seconds?: number;
  profile_id?: string;
  profile_name?: string;
};

export type CapabilityUsageSummary = {
  capability_id: string;
  used: boolean;
  group_enabled?: boolean;
  group_actor_count?: number;
  active_actor_count?: number;
  startup_autoload_actor_count?: number;
  actor_enabled?: CapabilityUsageActorEntry[];
  session_enabled?: CapabilityUsageActorEntry[];
  actor_autoload?: CapabilityUsageActorEntry[];
  profile_autoload?: CapabilityUsageActorEntry[];
  blocked?: boolean;
  blocked_scope?: string;
  blocked_reason?: string;
};

export type CapabilityImportRecord = {
  capability_id: string;
  kind: "mcp_toolpack" | "skill";
  install_mode?: "command" | "package" | "remote_only";
  install_spec?: { command?: string; package?: string; url?: string };
  name?: string;
  description_short?: string;
  use_when?: string[];
  avoid_when?: string[];
  gotchas?: string[];
  evidence_kind?: string;
  source_id?: string;
  [key: string]: unknown;
};

export type AgentStateHot = {
  active_task_id?: string | null;
  focus?: string | null;
  next_action?: string | null;
  blockers?: string[];
};

export type AgentStateWarm = {
  what_changed?: string | null;
  open_loops?: string[];
  commitments?: string[];
  environment_summary?: string | null;
  user_model?: string | null;
  persona_notes?: string | null;
  resume_hint?: string | null;
};

export type AgentState = {
  id: string;
  hot?: AgentStateHot | null;
  warm?: AgentStateWarm | null;
  updated_at?: string | null;
};

export type ActorRuntimeState = {
  id: string;
  runtime?: string;
  runner?: string;
  runner_effective?: string;
  running?: boolean;
  idle_seconds?: number | null;
  effective_working_state?: "stopped" | "idle" | "working" | "waiting" | "stuck" | string;
  effective_working_reason?: string;
  effective_working_updated_at?: string | null;
  effective_active_task_id?: string | null;
};

export type RuntimeInfo = {
  name: string;
  display_name: string;
  recommended_command?: string;
  available: boolean;
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

export type TaskStatus = "planned" | "active" | "done" | "archived" | string;

export type ChecklistStatus = "pending" | "in_progress" | "done" | string;

export type TaskWaitingOn = "none" | "user" | "actor" | "external" | string;

export type TaskChecklistItem = {
  id: string;
  text: string;
  status?: ChecklistStatus | null;
};

export type Task = {
  id: string;
  title?: string | null;
  outcome?: string | null;
  parent_id?: string | null;
  status?: TaskStatus | null;
  archived_from?: string | null;
  assignee?: string | null;
  priority?: string | null;
  blocked_by?: string[];
  waiting_on?: TaskWaitingOn | null;
  handoff_to?: string | null;
  task_type?: string | null;
  notes?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  checklist?: TaskChecklistItem[];
  steps?: TaskStep[];
  current_step?: string | null;
  progress?: number | null;
  children?: Task[];
};

export type CoordinationBrief = {
  objective?: string | null;
  current_focus?: string | null;
  constraints?: string[];
  project_brief?: string | null;
  project_brief_stale?: boolean;
  updated_by?: string | null;
  updated_at?: string | null;
};

export type CoordinationNote = {
  at?: string | null;
  by?: string | null;
  summary?: string | null;
  task_id?: string | null;
};

export type GroupTasksSummary = {
  total: number;
  done: number;
  active: number;
  planned: number;
  archived?: number;
  root_count?: number;
};

export type TaskBoardEntry = string | Partial<Task>;

export type PresentationCardType = "markdown" | "table" | "image" | "pdf" | "file" | "web_preview";

export type PresentationTableData = {
  columns: string[];
  rows: string[][];
};

export type PresentationContent = {
  mode?: "inline" | "reference" | "workspace_link";
  markdown?: string | null;
  table?: PresentationTableData | null;
  url?: string | null;
  blob_rel_path?: string | null;
  workspace_rel_path?: string | null;
  mime_type?: string | null;
  file_name?: string | null;
};

export type PresentationCard = {
  slot_id: string;
  title: string;
  card_type: PresentationCardType;
  published_by: string;
  published_at: string;
  source_label?: string;
  source_ref?: string;
  summary?: string;
  content: PresentationContent;
};

export type PresentationSlot = {
  slot_id: string;
  index: number;
  card?: PresentationCard | null;
};

export type GroupPresentation = {
  v: number;
  updated_at?: string;
  highlight_slot_id?: string;
  slots: PresentationSlot[];
};

export type PresentationBrowserSurfaceState = {
  active: boolean;
  state: string;
  message?: string | null;
  error?: {
    code?: string | null;
    message?: string | null;
  } | null;
  strategy?: string | null;
  url?: string | null;
  width?: number;
  height?: number;
  started_at?: string | null;
  updated_at?: string | null;
  last_frame_seq?: number;
  last_frame_at?: string | null;
  controller_attached?: boolean;
};

export type ContextAttention = {
  blocked?: number | TaskBoardEntry[];
  waiting_user?: number | TaskBoardEntry[];
  pending_handoffs?: number | TaskBoardEntry[];
};

export type ContextBoard = {
  planned?: TaskBoardEntry[];
  active?: TaskBoardEntry[];
  done?: TaskBoardEntry[];
  archived?: TaskBoardEntry[];
};

export type ContextDetailLevel = "summary" | "full";

export type GroupContext = {
  version?: string;
  coordination?: {
    brief?: CoordinationBrief | null;
    tasks?: Task[];
    recent_decisions?: CoordinationNote[];
    recent_handoffs?: CoordinationNote[];
  };
  agent_states?: AgentState[];
  actors_runtime?: ActorRuntimeState[];
  attention?: ContextAttention | null;
  board?: ContextBoard | null;
  tasks_summary?: GroupTasksSummary;
  meta?: {
    project_status?: string | null;
    [key: string]: unknown;
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

  desktop_pet_enabled: boolean;
};

export type BuiltinAssistantPolicy = {
  action_allowlist?: string[];
  requires_user_confirmation?: string[];
};

export type BuiltinAssistant = {
  assistant_id: string;
  kind: "pet" | "voice_secretary" | string;
  enabled: boolean;
  principal?: string;
  lifecycle: "disabled" | "idle" | "running" | "working" | "waiting" | "failed" | string;
  health?: Record<string, unknown>;
  policy?: BuiltinAssistantPolicy;
  config?: Record<string, unknown>;
  ui?: Record<string, unknown>;
};

export type AssistantServiceModel = {
  model_id: string;
  kind?: string;
  runtime_id?: string;
  title?: string;
  description?: string;
  status?: "not_installed" | "downloading" | "ready" | "failed" | "unknown" | string;
  available?: boolean;
  installed?: boolean;
  install_dir?: string;
  installed_at?: string;
  updated_at?: string;
  command_ready?: boolean;
  offline_ready?: boolean;
  streaming_ready?: boolean;
  offline?: Record<string, unknown>;
  streaming?: Record<string, unknown>;
  manifest_sha256?: string;
  downloaded_bytes?: number;
  total_size_bytes?: number;
  disk_usage_bytes?: number;
  progress_percent?: number;
  current_artifact_path?: string;
  artifact_index?: number;
  artifact_count?: number;
  error?: Record<string, unknown>;
  artifacts?: Array<{ path?: string; size_bytes?: number }>;
};

export type AssistantServiceRuntime = {
  runtime_id: string;
  status?: "not_installed" | "installing" | "ready" | "failed" | string;
  available?: boolean;
  installed?: boolean;
  install_dir?: string;
  python?: string;
  packages?: string[];
  modules?: Record<string, boolean>;
  missing_modules?: string[];
  installed_at?: string;
  updated_at?: string;
  disk_usage_bytes?: number;
  error?: Record<string, unknown>;
};

export type AssistantStateResult = {
  group_id: string;
  assistants?: BuiltinAssistant[];
  assistants_by_id?: Record<string, BuiltinAssistant>;
  assistant?: BuiltinAssistant;
  documents?: AssistantVoiceDocument[];
  documents_by_id?: Record<string, AssistantVoiceDocument>;
  active_document_id?: string;
  capture_target_document_id?: string;
  active_document_path?: string;
  capture_target_document_path?: string;
  documents_by_path?: Record<string, AssistantVoiceDocument>;
  new_input_available?: boolean;
  prompt_draft?: AssistantVoicePromptDraft;
  ask_requests?: AssistantVoiceAskFeedback[];
  latest_ask_request?: AssistantVoiceAskFeedback;
  service_models?: AssistantServiceModel[];
  service_models_by_id?: Record<string, AssistantServiceModel>;
  service_runtime?: AssistantServiceRuntime;
  service_runtimes?: AssistantServiceRuntime[];
  service_runtimes_by_id?: Record<string, AssistantServiceRuntime>;
};

export type AssistantVoiceTrigger = {
  schema?: number;
  trigger_kind?: string;
  mode?: string;
  capture_mode?: string;
  recognition_backend?: string;
  client_session_id?: string;
  input_device_label?: string;
  document_path?: string;
  language?: string;
  intent_hint?: string;
};

export type AssistantVoiceDocument = {
  document_id: string;
  document_path?: string;
  filename?: string;
  assistant_id?: string;
  title: string;
  status: "active" | "archived" | "deleted" | string;
  storage_kind?: "workspace" | "cccc_home" | string;
  workspace_path?: string;
  content?: string;
  content_sha256?: string;
  content_chars?: number;
  revision_count?: number;
  source_segment_count?: number;
  last_source_segment_id?: string;
  last_source_path?: string;
  created_at?: string;
  updated_at?: string;
  created_by?: string;
};

export type AssistantVoiceTranscriptionResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  transcript: string;
  mime_type?: string;
  language?: string;
  bytes?: number;
  backend?: string;
  service?: Record<string, unknown>;
  asr?: Record<string, unknown>;
};

export type AssistantVoiceTranscriptSegmentResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  session_id: string;
  segment?: Record<string, unknown>;
  segment_path?: string;
  document?: AssistantVoiceDocument;
  document_updated?: boolean;
  input_event?: Record<string, unknown>;
  input_event_created?: boolean;
  input_notify_emitted?: boolean;
  input_notify_error?: string;
  actor_woken?: boolean;
  actor_wake_error?: string;
  actor_notify_delivered?: boolean;
  actor_notify_delivery_error?: string;
};

export type AssistantVoiceMeetingSession = {
  schema?: number;
  group_id?: string;
  session_id: string;
  status?: string;
  created_at?: string;
  updated_at?: string;
  sample_rate?: number;
  audio_duration_ms?: number;
  language?: string;
  document_path?: string;
  latest_partial?: string;
  last_final_text?: string;
  segments?: Record<string, unknown>[];
  error?: Record<string, unknown> | null;
};

export type AssistantVoiceDocumentMutationResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  document?: AssistantVoiceDocument;
  input_event?: Record<string, unknown>;
  input_event_created?: boolean;
  input_notify_emitted?: boolean;
  input_notify_error?: string;
  actor_woken?: boolean;
  actor_wake_error?: string;
  actor_notify_delivered?: boolean;
  actor_notify_delivery_error?: string;
  event?: unknown;
  request_id?: string;
};

export type AssistantVoicePromptDraft = {
  request_id: string;
  status: "pending" | "applied" | "dismissed" | "stale" | string;
  operation?: string;
  draft_text: string;
  draft_preview?: string;
  summary?: string;
  composer_snapshot_hash?: string;
  created_at?: string;
  updated_at?: string;
};

export type AssistantVoiceAskFeedback = {
  request_id: string;
  status: "pending" | "working" | "done" | "needs_user" | "failed" | "handed_off" | string;
  request_text?: string;
  request_preview?: string;
  reply_text?: string;
  document_path?: string;
  artifact_paths?: string[];
  source_summary?: string;
  checked_at?: string;
  source_urls?: string[];
  target_kind?: string;
  intent_hint?: string;
  language?: string;
  handoff_target?: string;
  handoff_request_id?: string;
  target_actor_id?: string;
  created_at?: string;
  updated_at?: string;
  cleared_at?: string;
};

export type AssistantVoiceInputResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  document?: AssistantVoiceDocument;
  input_event?: Record<string, unknown>;
  input_event_created?: boolean;
  input_notify_emitted?: boolean;
  input_notify_error?: string;
  actor_woken?: boolean;
  actor_wake_error?: string;
  actor_notify_delivered?: boolean;
  actor_notify_delivery_error?: string;
  event?: unknown;
  request_id?: string;
};

export type AssistantVoicePromptDraftMutationResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  prompt_draft?: AssistantVoicePromptDraft;
  event?: unknown;
};

export type RemoteAccessState = {
  provider: "off" | "manual" | "tailscale" | string;
  mode: string;
  require_access_token: boolean;
  enabled: boolean;
  status: "stopped" | "running" | "not_installed" | "not_authenticated" | "misconfigured" | "error" | string;
  status_reason?: "local_only" | "apply_pending" | "missing_access_token" | "binding_unreachable" | "unsupported_mode" | "provider_not_installed" | "provider_not_authenticated" | "provider_error" | "stopped" | "running" | "misconfigured" | "unknown" | string;
  endpoint?: string | null;
  updated_at?: string | null;
  restart_required?: boolean;
  apply_supported?: boolean;
  diagnostics?: {
    access_token_present?: boolean;
    access_token_requirement_satisfied?: boolean;
    access_token_source?: "store" | "none" | string;
    access_token_count?: number;
    allow_insecure_remote_override?: boolean;
    effective_require_access_token?: boolean;
    exposure_class?: "local" | "private" | "public" | string;
    running_in_wsl?: boolean;
    web_host?: string;
    web_host_source?: "settings" | "env" | "default" | string;
    web_port?: number;
    web_port_source?: "settings" | "env" | "default" | string;
    web_public_url?: string | null;
    web_public_url_source?: "settings" | "env" | "none" | string;
    web_bind_loopback?: boolean;
    web_bind_reachable?: boolean;
    mode_supported?: boolean;
    desired_local_url?: string | null;
    desired_remote_url?: string | null;
    live_runtime_present?: boolean;
    live_runtime_pid?: number | null;
    live_runtime_host?: string | null;
    live_runtime_port?: number | null;
    live_runtime_mode?: string | null;
    live_runtime_supervisor_managed?: boolean;
    live_runtime_matches_binding?: boolean;
    live_local_url?: string | null;
    live_remote_url?: string | null;
    apply_supported?: boolean;
    last_apply_error?: string | null;
    tailscale_installed?: boolean | null;
    tailscale_backend_state?: string | null;
  } | null;
  config?: {
    web_host?: string;
    web_port?: number;
    web_public_url?: string | null;
    access_token_configured?: boolean;
    access_token_count?: number;
    access_token_source?: "store" | "none" | string;
  } | null;
  next_steps?: string[] | null;
};

export type WebAccessSession = {
  login_active: boolean;
  current_browser_signed_in: boolean;
  principal_kind?: string;
  user_id?: string | null;
  is_admin?: boolean;
  allowed_groups?: string[];
  access_token_count?: number;
  can_access_global_settings?: boolean;
  runtime_visibility?: {
    peer_runtime?: "hidden" | "visible" | string;
    pet_runtime?: "hidden" | "visible" | string;
  };
};

export type WebBranding = {
  product_name: string;
  logo_icon_url?: string | null;
  favicon_url?: string | null;
  has_custom_logo_icon?: boolean;
  has_custom_favicon?: boolean;
  updated_at?: string | null;
};

export type GroupSpaceProviderState = {
  provider: "notebooklm" | string;
  enabled: boolean;
  real_enabled?: boolean;
  mode: "disabled" | "active" | "degraded" | string;
  last_health_at?: string | null;
  last_error?: string | null;
  real_adapter_enabled?: boolean;
  stub_adapter_enabled?: boolean;
  auth_configured?: boolean;
  write_ready?: boolean;
  readiness_reason?: string;
};

export type GroupSpaceProviderCredentialStatus = {
  provider: "notebooklm" | string;
  key: string;
  configured: boolean;
  source: "none" | "store" | "env" | string;
  env_configured: boolean;
  store_configured: boolean;
  updated_at?: string | null;
  masked_value?: string | null;
};

export type GroupSpaceProviderAuthState =
  | "idle"
  | "running"
  | "succeeded"
  | "failed"
  | "canceled"
  | string;

export type GroupSpaceProviderAuthStatus = {
  provider: "notebooklm" | string;
  state: GroupSpaceProviderAuthState;
  phase?: string;
  delivery?: "local_browser" | "projected_browser" | string;
  session_id?: string;
  started_at?: string;
  updated_at?: string;
  finished_at?: string;
  message?: string;
  error?: { code?: string; message?: string } | null;
  projected_browser?: PresentationBrowserSurfaceState | null;
};

export type GroupSpaceLane = "work" | "memory" | string;

export type GroupSpaceBinding = {
  group_id: string;
  provider: "notebooklm" | string;
  lane?: GroupSpaceLane;
  remote_space_id?: string;
  bound_by?: string;
  bound_at?: string;
  status: "bound" | "unbound" | "error" | string;
};

export type GroupSpaceRemoteSpace = {
  remote_space_id: string;
  title?: string;
  created_at?: string;
  is_owner?: boolean;
};

export type GroupSpaceSource = {
  source_id: string;
  title?: string;
  url?: string;
  status?: number | string;
  kind?: string;
};

export type GroupSpaceArtifact = {
  artifact_id: string;
  title?: string;
  kind?: string;
  status?: string;
  created_at?: string;
  url?: string;
};

export type GroupSpaceQueueSummary = {
  pending: number;
  running: number;
  failed: number;
};

export type GroupSpaceMemorySyncSummary = {
  lane: "memory" | string;
  manifest_path: string;
  last_scan_at?: string | null;
  last_success_at?: string | null;
  pending_files: number;
  running_files: number;
  failed_files: number;
  blocked_files: number;
};

export type GroupSpaceJobError = {
  code?: string;
  message?: string;
};

export type GroupSpaceJob = {
  job_id: string;
  group_id: string;
  provider: "notebooklm" | string;
  lane?: GroupSpaceLane;
  remote_space_id: string;
  kind: "context_sync" | "resource_ingest" | string;
  payload: Record<string, unknown>;
  payload_digest?: string;
  idempotency_key?: string;
  state: "pending" | "running" | "succeeded" | "failed" | "canceled" | string;
  attempt: number;
  max_attempts: number;
  next_run_at?: string | null;
  created_at?: string;
  updated_at?: string;
  last_error?: GroupSpaceJobError;
};

export type GroupSpaceStatus = {
  group_id: string;
  provider: GroupSpaceProviderState;
  bindings: Record<string, GroupSpaceBinding>;
  queue_summary: Record<string, GroupSpaceQueueSummary>;
  sync?: GroupSpaceSyncState;
  memory_sync?: GroupSpaceMemorySyncSummary;
  sync_result?: GroupSpaceSyncResult | Record<string, unknown>;
};

export type GroupSpaceSyncState = {
  available?: boolean;
  reason?: string;
  space_root?: string;
  group_id?: string;
  provider?: string;
  remote_space_id?: string;
  last_run_at?: string;
  converged?: boolean;
  unsynced_count?: number;
  uploaded?: number;
  updated?: number;
  deleted?: number;
  reused?: number;
  last_error?: string;
  last_fingerprint?: Record<string, unknown>;
  errors?: Array<Record<string, unknown>>;
};

export type GroupSpaceSyncResult = {
  ok?: boolean;
  group_id?: string;
  provider?: string;
  remote_space_id?: string;
  space_root?: string;
  skipped?: boolean;
  reason?: string;
  converged?: boolean;
  unsynced_count?: number;
  local_files?: number;
  uploaded?: number;
  updated?: number;
  deleted?: number;
  reused?: number;
  errors?: Array<Record<string, unknown>>;
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

export type AutomationSnippetCatalog = {
  built_in: Record<string, string>;
  built_in_overrides: Record<string, string>;
  custom: Record<string, string>;
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
  snippet_catalog?: AutomationSnippetCatalog;
  status: Record<string, AutomationRuleStatus>;
  config_path: string;
  supported_vars: string[];
  version?: number;
  server_now?: string;
};

export type IMPlatform = "telegram" | "slack" | "discord" | "feishu" | "dingtalk" | "wecom" | "weixin";

export type IMConfig = {
  platform?: IMPlatform;
  // Legacy single token field (backward compat)
  token_env?: string;
  token?: string;
  // Canonical token fields
  bot_token?: string;
  app_token?: string;
  // Token env fields (Slack/Telegram/Discord)
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
  // WeCom fields
  wecom_bot_id?: string;
  wecom_secret?: string;
  // Weixin fields
  weixin_account_id?: string;
};

export type IMStatus = {
  group_id: string;
  configured: boolean;
  enabled: boolean;
  platform?: string;
  running: boolean;
  pid?: number;
  subscribers: number;
};

export type WeixinLoginStatus = {
  status: string;
  logged_in: boolean;
  account_id?: string;
  qrcode_url?: string;
  qr_ascii?: string;
  error?: string;
  running: boolean;
  pid?: number | null;
  updated_at?: string;
};

export type DirItem = { name: string; path: string; is_dir: boolean };
export type DirSuggestion = { name: string; path: string; icon: string };
export type PresentationWorkspaceItem = { name: string; path: string; is_dir: boolean; mime_type?: string | null };
export type PresentationWorkspaceListing = {
  root_path: string;
  path: string;
  parent: string | null;
  items: PresentationWorkspaceItem[];
};

// Runtime configuration
export const SUPPORTED_RUNTIMES = [
  "claude",
  "codex",
  "droid",
  "amp",
  "auggie",
  "neovate",
  "gemini",
  "kimi",
  "web_model",
  "custom",
] as const;

export type SupportedRuntime = typeof SUPPORTED_RUNTIMES[number];

export const RUNTIME_INFO: Record<string, { label: string; desc: string }> = {
  amp: { label: "Amp", desc: "" },
  auggie: { label: "Auggie (Augment)", desc: "" },
  claude: { label: "Claude Code", desc: "" },
  codex: { label: "Codex CLI", desc: "" },
  droid: { label: "Droid", desc: "" },
  gemini: { label: "Gemini CLI", desc: "" },
  kimi: { label: "Kimi CLI", desc: "" },
  neovate: { label: "Neovate Code", desc: "" },
  web_model: { label: "ChatGPT Web Model", desc: "ChatGPT browser delivery + remote MCP connector" },
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
  droid: { 
    bg: "bg-violet-900/30", text: "text-violet-300", border: "border-violet-600/50", dot: "bg-violet-400",
    bgLight: "bg-violet-50", textLight: "text-violet-700", borderLight: "border-violet-300", dotLight: "bg-violet-500"
  },
  gemini: {
    bg: "bg-yellow-900/30", text: "text-yellow-300", border: "border-yellow-600/50", dot: "bg-yellow-400",
    bgLight: "bg-yellow-50", textLight: "text-yellow-700", borderLight: "border-yellow-300", dotLight: "bg-yellow-500"
  },
  kimi: {
    bg: "bg-lime-900/30", text: "text-lime-300", border: "border-lime-600/50", dot: "bg-lime-400",
    bgLight: "bg-lime-50", textLight: "text-lime-700", borderLight: "border-lime-300", dotLight: "bg-lime-500"
  },
  neovate: {
    bg: "bg-fuchsia-900/30", text: "text-fuchsia-300", border: "border-fuchsia-600/50", dot: "bg-fuchsia-400",
    bgLight: "bg-fuchsia-50", textLight: "text-fuchsia-700", borderLight: "border-fuchsia-300", dotLight: "bg-fuchsia-500"
  },
  web_model: {
    bg: "bg-indigo-900/30", text: "text-indigo-300", border: "border-indigo-600/50", dot: "bg-indigo-400",
    bgLight: "bg-indigo-50", textLight: "text-indigo-700", borderLight: "border-indigo-300", dotLight: "bg-indigo-500"
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
