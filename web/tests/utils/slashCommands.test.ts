import { describe, expect, it } from "vitest";

import {
  buildSlashCommandToolArguments,
  buildSlashCommandToolArgumentsForItem,
  buildSlashCommandsFromCapabilityState,
  filterSlashCommands,
  parseSlashCommandInput,
} from "../../src/utils/slashCommands";

describe("slashCommands", () => {
  it("prefers real tool names when building slash commands", () => {
    const commands = buildSlashCommandsFromCapabilityState({
      group_id: "g1",
      actor_id: "user",
      enabled: [],
      dynamic_tools: [
        {
          name: "cccc_ext_deadbeef_echo",
          capability_id: "mcp:test-server",
          description: "Echo tool",
          real_tool_name: "echo",
          inputSchema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
        },
      ],
    } as any);

    expect(commands).toEqual([
      expect.objectContaining({
        name: "echo",
        command: "/echo",
        toolName: "cccc_ext_deadbeef_echo",
        realToolName: "echo",
        inputSchema: expect.objectContaining({ required: ["message"] }),
      }),
    ]);
  });

  it("filters slash commands by name and description", () => {
    const commands = buildSlashCommandsFromCapabilityState({
      group_id: "g1",
      actor_id: "user",
      enabled: [],
      dynamic_tools: [
        { name: "resolve_library_id", capability_id: "mcp:context7", description: "Resolve library id" },
        { name: "echo", capability_id: "mcp:test-server", description: "Echo tool" },
      ],
    } as any);

    expect(filterSlashCommands(commands, "/res").map((item) => item.name)).toEqual(["resolve_library_id"]);
    expect(filterSlashCommands(commands, "/library").map((item) => item.name)).toEqual(["resolve_library_id"]);
  });

  it("parses slash command input and preserves argument text", () => {
    const commands = buildSlashCommandsFromCapabilityState({
      group_id: "g1",
      actor_id: "user",
      enabled: [],
      dynamic_tools: [{ name: "superpowers", capability_id: "mcp:superpowers", description: "Run superpowers" }],
    } as any);

    expect(parseSlashCommandInput("/superpowers summarize repo", commands)).toEqual({
      item: expect.objectContaining({ name: "superpowers" }),
      commandText: "superpowers",
      argsText: "summarize repo",
    });
    expect(parseSlashCommandInput("/unknown test", commands)).toBeNull();
  });

  it("builds common text-first tool arguments for MVP execution", () => {
    expect(buildSlashCommandToolArguments("summarize repo")).toEqual({
      text: "summarize repo",
      input: "summarize repo",
      query: "summarize repo",
      prompt: "summarize repo",
      message: "summarize repo",
    });
    expect(buildSlashCommandToolArguments("")).toEqual({});
  });

  it("uses schema required fields when building tool arguments", () => {
    const [command] = buildSlashCommandsFromCapabilityState({
      group_id: "g1",
      actor_id: "user",
      enabled: [],
      dynamic_tools: [
        {
          name: "resolve_library_id",
          capability_id: "mcp:context7",
          description: "Resolve library id",
          inputSchema: {
            type: "object",
            properties: { libraryName: { type: "string" } },
            required: ["libraryName"],
          },
        },
      ],
    } as any);

    expect(buildSlashCommandToolArgumentsForItem(command!, "react")).toEqual({ libraryName: "react" });
  });
});
