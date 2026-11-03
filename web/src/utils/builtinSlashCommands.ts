import type { SlashCommandItem } from "./slashCommands";

export const BUILTIN_SLASH_COMMANDS: SlashCommandItem[] = [
  {
    name: "install",
    command: "/install",
    description: "Install or enable a skill, MCP toolpack, capability, URL, repo, or local path.",
    capabilityId: "skill:cccc:install",
    sourceType: "builtin_command",
  },
];

/**
 * CCCC's own skills, described in the reader's language while they carry the built-in description
 * (the English text in chat.json). A skill imported under one of these ids shows its own
 * description, unless that is word for word the built-in one.
 */
export const BUILTIN_SLASH_DESCRIPTION_KEYS = new Map([
  ["skill:cccc:install", "slashDescriptions.install"],
  ["skill:cccc:self-evolution", "slashDescriptions.selfEvolution"],
]);
