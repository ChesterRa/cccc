import type { CapabilityStateResult } from "../types";
import { BUILTIN_SLASH_COMMANDS } from "./builtinSlashCommands";

export type SlashCommandItem = {
  name: string;
  command: string;
  description?: string;
  capabilityId: string;
  toolName?: string;
  realToolName?: string;
  inputSchema?: Record<string, unknown>;
  sourceType: "dynamic_tool" | "capsule_skill" | "builtin_command";
  active?: boolean;
};

export type ParsedSlashCommand = {
  item: SlashCommandItem;
  commandText: string;
  argsText: string;
};

export type SlashCommandDisplayKind = "command" | "skill" | "tool";

export type CapsuleSkillSlashCommandResolution =
  | { kind: "dispatch"; dispatchText: string }
  | { kind: "missing_args"; message: string }
  | { kind: "not_capsule_skill" };

export type SlashCommandGuardInput = {
  composerFilesCount: number;
  hasReplyTarget: boolean;
  hasQuotedPresentationRef: boolean;
  sendGroupId: string;
  selectedGroupId: string;
};

export type SlashCommandGuardResult =
  | { ok: true }
  | { ok: false; message: string };

export type SlashCommandGuardMessages = {
  attachmentsUnsupported: string;
  repliesUnsupported: string;
  quotedPresentationUnsupported: string;
  crossGroupUnsupported: string;
};

export type CapsuleSkillSlashCommandMessages = {
  missingArgs: (command: string) => string;
};

const DEFAULT_SLASH_GUARD_MESSAGES: SlashCommandGuardMessages = {
  attachmentsUnsupported: "Slash command does not support attachments.",
  repliesUnsupported: "Slash command does not support replies.",
  quotedPresentationUnsupported: "Slash command does not support quoted presentation views.",
  crossGroupUnsupported: "Slash command does not support cross-group send.",
};

export function slashCommandDisplayKind(item: Pick<SlashCommandItem, "sourceType" | "capabilityId">): SlashCommandDisplayKind {
  if (String(item.capabilityId || "").trim().startsWith("skill:")) return "skill";
  if (item.sourceType === "dynamic_tool") return "tool";
  return "command";
}

function normalizeCommandToken(value: unknown): string {
  const text = String(value || "").trim().toLowerCase();
  if (!text) return "";
  return text
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function capabilityIdTail(value: unknown): string {
  const text = String(value || "").trim();
  if (!text) return "";
  const parts = text.split(":").map((part) => part.trim()).filter(Boolean);
  return parts[parts.length - 1] || text;
}

function uniqueCommandName(candidates: unknown[], used: Set<string>): string {
  for (const candidate of candidates) {
    const normalized = normalizeCommandToken(candidate);
    if (normalized && !used.has(normalized)) return normalized;
  }
  return "";
}

function sortSlashCommands(commands: SlashCommandItem[]): SlashCommandItem[] {
  return commands.slice().sort((a, b) => {
    if (a.sourceType !== b.sourceType) {
      if (a.sourceType === "builtin_command") return -1;
      if (b.sourceType === "builtin_command") return 1;
    }
    if (a.sourceType !== b.sourceType) {
      if (a.sourceType === "capsule_skill") return -1;
      if (b.sourceType === "capsule_skill") return 1;
    }
    return a.name.localeCompare(b.name);
  });
}

export function buildSlashCommands(args: {
  state?: CapabilityStateResult | null;
}): SlashCommandItem[] {
  const used = new Set<string>();
  const commands: SlashCommandItem[] = [];
  const actorHiddenCapabilityIds = new Set(
    (Array.isArray(args.state?.actor_hidden_capabilities) ? args.state.actor_hidden_capabilities : [])
      .map((item) => String(item || "").trim())
      .filter(Boolean),
  );
  for (const command of BUILTIN_SLASH_COMMANDS) {
    const name = uniqueCommandName([command.name, command.command], used);
    if (!name) continue;
    used.add(name);
    commands.push({ ...command, name, command: `/${name}` });
  }

  const activeSkills = Array.isArray(args.state?.active_capsule_skills) ? args.state.active_capsule_skills : [];
  for (const skill of activeSkills) {
    if (!skill || typeof skill !== "object") continue;
    const capabilityId = String(skill.capability_id || "").trim();
    if (!capabilityId || actorHiddenCapabilityIds.has(capabilityId) || commands.some((item) => item.capabilityId === capabilityId)) continue;
    const name = uniqueCommandName([skill.name, capabilityIdTail(capabilityId), capabilityId], used);
    if (!name) continue;
    used.add(name);
    commands.push({
      name,
      command: `/${name}`,
      description: String(skill.description_short || skill.capsule_preview || "").trim() || undefined,
      capabilityId,
      sourceType: "capsule_skill",
      active: true,
    });
  }

  const dynamicTools = Array.isArray(args.state?.dynamic_tools) ? args.state.dynamic_tools : [];
  for (const tool of dynamicTools) {
    if (!tool || typeof tool !== "object") continue;
    const capabilityId = String(tool.capability_id || "").trim();
    const toolName = String(tool.name || "").trim();
    if (!capabilityId || !toolName) continue;
    const realToolName = String(tool.real_tool_name || "").trim();
    const inputSchema = tool.inputSchema && typeof tool.inputSchema === "object" ? tool.inputSchema : undefined;
    if (!supportsSlashTextToolArguments(inputSchema)) continue;
    const name = uniqueCommandName([realToolName, toolName], used);
    if (!name) continue;
    used.add(name);
    commands.push({
      name,
      command: `/${name}`,
      description: String(tool.description || "").trim() || undefined,
      capabilityId,
      toolName,
      realToolName: realToolName || undefined,
      inputSchema,
      sourceType: "dynamic_tool",
    });
  }

  return sortSlashCommands(commands);
}

export function filterSlashCommands(commands: SlashCommandItem[], input: string): SlashCommandItem[] {
  const text = String(input || "");
  if (text !== text.trimStart()) return [];
  if (!text.startsWith("/")) return [];
  const firstToken = text.slice(1).split(/\s+/, 1)[0] || "";
  const query = normalizeCommandToken(firstToken);
  if (!query) return commands.slice();
  return commands
    .filter((item) => {
      const haystacks = [item.name, item.toolName || "", item.realToolName || "", item.description || ""]
        .map((value) => value.toLowerCase());
      return haystacks.some((value) => value.includes(query));
    });
}

export function getVisibleSlashCommandPage(commands: SlashCommandItem[], visibleCount: number): SlashCommandItem[] {
  const count = Math.max(0, Math.floor(Number(visibleCount) || 0));
  return commands.slice(0, count);
}

export function parseSlashCommandInput(text: string, commands: SlashCommandItem[]): ParsedSlashCommand | null {
  const raw = String(text || "");
  if (raw !== raw.trimStart()) return null;
  const trimmed = raw.trim();
  if (!trimmed.startsWith("/")) return null;
  const spaceIndex = trimmed.indexOf(" ");
  const commandText = spaceIndex >= 0 ? trimmed.slice(1, spaceIndex) : trimmed.slice(1);
  const argsText = spaceIndex >= 0 ? trimmed.slice(spaceIndex + 1).trim() : "";
  const normalized = normalizeCommandToken(commandText);
  if (!normalized) return null;
  const item = commands.find((command) => command.name === normalized);
  if (!item) return null;
  return { item, commandText: normalized, argsText };
}

export function resolveSlashCommandGuard(
  input: SlashCommandGuardInput,
  messages: Partial<SlashCommandGuardMessages> = {},
): SlashCommandGuardResult {
  const copy = { ...DEFAULT_SLASH_GUARD_MESSAGES, ...messages };
  if (input.composerFilesCount > 0) {
    return { ok: false, message: copy.attachmentsUnsupported };
  }
  if (input.hasReplyTarget) {
    return { ok: false, message: copy.repliesUnsupported };
  }
  if (input.hasQuotedPresentationRef) {
    return { ok: false, message: copy.quotedPresentationUnsupported };
  }
  const sendGroupId = String(input.sendGroupId || "").trim();
  const selectedGroupId = String(input.selectedGroupId || "").trim();
  if (sendGroupId && sendGroupId !== selectedGroupId) {
    return { ok: false, message: copy.crossGroupUnsupported };
  }
  return { ok: true };
}

export function buildCapsuleSkillDispatchText(item: SlashCommandItem, argsText: string): string {
  if (item.sourceType !== "capsule_skill") return "";
  const text = String(argsText || "").trim();
  if (!text) return "";
  const skillLabel = String(item.command || item.name || "").trim();
  return `请使用已激活的 ${skillLabel} skill 完成以下任务：\n\n${text}`;
}

export function resolveCapsuleSkillSlashCommand(
  item: SlashCommandItem,
  argsText: string,
  messages: Partial<CapsuleSkillSlashCommandMessages> = {},
): CapsuleSkillSlashCommandResolution {
  if (item.sourceType !== "capsule_skill") return { kind: "not_capsule_skill" };
  const dispatchText = buildCapsuleSkillDispatchText(item, argsText);
  if (dispatchText) return { kind: "dispatch", dispatchText };
  const skillLabel = String(item.command || item.name || "the skill").trim();
  return {
    kind: "missing_args",
    message: messages.missingArgs ? messages.missingArgs(skillLabel) : `Enter a task after ${skillLabel}.`,
  };
}

function schemaProperties(schema: Record<string, unknown> | undefined): Record<string, unknown> {
  const properties = schema?.properties;
  return properties && typeof properties === "object" && !Array.isArray(properties)
    ? properties as Record<string, unknown>
    : {};
}

function schemaFieldType(schema: Record<string, unknown> | undefined, field: string): string {
  const row = schemaProperties(schema)[field];
  if (!row || typeof row !== "object" || Array.isArray(row)) return "";
  const type = (row as Record<string, unknown>).type;
  if (Array.isArray(type)) return type.map((item) => String(item || "").trim()).filter(Boolean).join("|");
  return String(type || "").trim();
}

function isTextCompatibleSchemaField(schema: Record<string, unknown> | undefined, field: string): boolean {
  const type = schemaFieldType(schema, field);
  return !type || type.split("|").includes("string");
}

function preferredTextFieldFromSchema(schema: Record<string, unknown> | undefined): string {
  const properties = schemaProperties(schema);
  const required = Array.isArray(schema?.required)
    ? schema.required.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
  const preferred = ["text", "input", "query", "prompt", "message", "libraryName", "name", "content"];
  for (const key of required) {
    if (Object.prototype.hasOwnProperty.call(properties, key)) return key;
  }
  for (const key of preferred) {
    if (Object.prototype.hasOwnProperty.call(properties, key)) return key;
  }
  return "";
}

export function supportsSlashTextToolArguments(schema: Record<string, unknown> | undefined): boolean {
  if (!schema || Object.keys(schema).length === 0) return true;
  const properties = schemaProperties(schema);
  const required = Array.isArray(schema.required)
    ? schema.required.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
  const textField = preferredTextFieldFromSchema(schema);
  if (!textField || !isTextCompatibleSchemaField(schema, textField)) return false;
  const nonTextRequired = required.filter((field) => field !== textField);
  if (nonTextRequired.length > 0) return false;
  const additionalProperties = schema.additionalProperties;
  if (additionalProperties === false && textField && !Object.prototype.hasOwnProperty.call(properties, textField)) {
    return false;
  }
  return true;
}

export function buildSlashCommandToolArgumentsForItem(
  item: SlashCommandItem,
  argsText: string,
): Record<string, unknown> {
  if (item.sourceType !== "dynamic_tool") return {};
  const text = String(argsText || "").trim();
  if (!text) return {};
  if (!supportsSlashTextToolArguments(item.inputSchema)) return {};
  const schemaField = preferredTextFieldFromSchema(item.inputSchema);
  if (schemaField) return { [schemaField]: text };
  return { text, input: text, query: text, prompt: text, message: text };
}
