import type { CapabilityStateResult } from "../types";

export type SlashCommandItem = {
  name: string;
  command: string;
  description?: string;
  capabilityId?: string;
  toolName: string;
  realToolName?: string;
  inputSchema?: Record<string, unknown>;
  sourceType: "dynamic_tool";
};

export type ParsedSlashCommand = {
  item: SlashCommandItem;
  commandText: string;
  argsText: string;
};

function normalizeCommandToken(value: unknown): string {
  const text = String(value || "").trim().toLowerCase();
  if (!text) return "";
  return text
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function buildCommandCandidates(name: string, realToolName?: string): string[] {
  const unique = new Set<string>();
  for (const candidate of [normalizeCommandToken(realToolName), normalizeCommandToken(name)]) {
    if (candidate) unique.add(candidate);
  }
  return [...unique];
}

export function buildSlashCommandsFromCapabilityState(state: CapabilityStateResult | null | undefined): SlashCommandItem[] {
  const dynamicTools = Array.isArray(state?.dynamic_tools) ? state.dynamic_tools : [];
  const used = new Set<string>();
  const commands: SlashCommandItem[] = [];

  for (const tool of dynamicTools) {
    if (!tool || typeof tool !== "object") continue;
    const toolName = String(tool.name || "").trim();
    if (!toolName) continue;
    const capabilityId = String(tool.capability_id || "").trim();
    const realToolName = String(tool.real_tool_name || "").trim();
    const commandName = buildCommandCandidates(toolName, realToolName).find((candidate) => !used.has(candidate)) || "";
    if (!commandName) continue;
    used.add(commandName);
    commands.push({
      name: commandName,
      command: `/${commandName}`,
      description: String(tool.description || "").trim() || undefined,
      capabilityId: capabilityId || undefined,
      toolName,
      realToolName: realToolName || undefined,
      inputSchema: tool.inputSchema && typeof tool.inputSchema === "object" ? tool.inputSchema : undefined,
      sourceType: "dynamic_tool",
    });
  }

  return commands.sort((a, b) => a.name.localeCompare(b.name));
}

export function filterSlashCommands(commands: SlashCommandItem[], input: string): SlashCommandItem[] {
  const text = String(input || "").trimStart();
  if (!text.startsWith("/")) return [];
  const query = normalizeCommandToken(text.slice(1).split(/\s+/, 1)[0] || "");
  if (!query) return commands.slice(0, 8);
  return commands
    .filter((item) => {
      const haystacks = [item.name, item.toolName, item.realToolName || "", item.description || ""]
        .map((value) => String(value || "").toLowerCase());
      return haystacks.some((value) => value.includes(query));
    })
    .slice(0, 8);
}

export function parseSlashCommandInput(text: string, commands: SlashCommandItem[]): ParsedSlashCommand | null {
  const trimmed = String(text || "").trim();
  if (!trimmed.startsWith("/")) return null;
  const spaceIndex = trimmed.indexOf(" ");
  const commandText = spaceIndex >= 0 ? trimmed.slice(1, spaceIndex) : trimmed.slice(1);
  const argsText = spaceIndex >= 0 ? trimmed.slice(spaceIndex + 1).trim() : "";
  const normalized = normalizeCommandToken(commandText);
  if (!normalized) return null;
  const item = commands.find((command) => command.name === normalized);
  if (!item) return null;
  return {
    item,
    commandText: normalized,
    argsText,
  };
}

export function buildSlashCommandToolArguments(argsText: string): Record<string, unknown> {
  const text = String(argsText || "").trim();
  if (!text) return {};
  return {
    text,
    input: text,
    query: text,
    prompt: text,
    message: text,
  };
}

function schemaProperties(schema: Record<string, unknown> | undefined): Record<string, unknown> {
  const properties = schema?.properties;
  return properties && typeof properties === "object" && !Array.isArray(properties)
    ? properties as Record<string, unknown>
    : {};
}

function preferredTextFieldFromSchema(schema: Record<string, unknown> | undefined): string {
  const properties = schemaProperties(schema);
  const required = Array.isArray(schema?.required)
    ? schema.required.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
  const preferred = [
    "text",
    "input",
    "query",
    "prompt",
    "message",
    "libraryName",
    "name",
    "content",
  ];
  for (const key of required) {
    if (Object.prototype.hasOwnProperty.call(properties, key)) return key;
  }
  for (const key of preferred) {
    if (Object.prototype.hasOwnProperty.call(properties, key)) return key;
  }
  return "";
}

export function buildSlashCommandToolArgumentsForItem(
  item: SlashCommandItem,
  argsText: string,
): Record<string, unknown> {
  const text = String(argsText || "").trim();
  if (!text) return {};
  const schemaField = preferredTextFieldFromSchema(item.inputSchema);
  if (schemaField) {
    return { [schemaField]: text };
  }
  return {
    text,
    input: text,
    query: text,
    prompt: text,
    message: text,
  };
}
