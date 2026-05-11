import { describe, expect, it } from "vitest";

import {
  buildCapsuleSkillDispatchText,
  buildSlashCommandToolArgumentsForItem,
  buildSlashCommands,
  filterSlashCommands,
  getVisibleSlashCommandPage,
  parseSlashCommandInput,
  resolveSlashCommandGuard,
  resolveCapsuleSkillSlashCommand,
  slashCommandDisplayKind,
} from "../../src/utils/slashCommands";

describe("slashCommands", () => {
  it("always includes the builtin install command", () => {
    const commands = buildSlashCommands({ state: null });

    expect(commands[0]).toMatchObject({
      command: "/install",
      capabilityId: "skill:cccc:install",
      sourceType: "builtin_command",
    });
    expect(parseSlashCommandInput("/install https://github.com/obra/superpowers", commands)?.argsText).toBe(
      "https://github.com/obra/superpowers",
    );
    expect(filterSlashCommands(commands, "/ins").map((item) => item.command)).toContain("/install");
  });

  it("labels builtin skill-backed commands as skills in the slash menu", () => {
    const commands = buildSlashCommands({ state: null });

    expect(slashCommandDisplayKind(commands[0])).toBe("skill");
  });

  it("builds skill commands only from active runtime skill state", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [
          {
            capability_id: "skill:agent_self_proposed:active-review",
            name: "active-review",
            description_short: "Already active",
          },
        ],
        dynamic_tools: [],
      },
    });

    expect(commands.map((item) => item.command)).toEqual(["/install", "/active-review"]);
    expect(commands.find((item) => item.command === "/active-review")?.sourceType).toBe("capsule_skill");
  });

  it("does not build slash commands for actor-hidden runtime skills", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        actor_hidden_capabilities: ["skill:team:hidden-writer"],
        active_capsule_skills: [
          {
            capability_id: "skill:team:visible-reviewer",
            name: "visible-reviewer",
            description_short: "Visible skill",
          },
          {
            capability_id: "skill:team:hidden-writer",
            name: "hidden-writer",
            description_short: "Hidden skill",
          },
        ],
        dynamic_tools: [],
      },
    });

    expect(commands.map((item) => item.command)).toEqual(["/install", "/visible-reviewer"]);
    expect(parseSlashCommandInput("/hidden-writer draft this", commands)).toBeNull();
  });

  it("keeps the builtin install command as the only slash entry for its capability", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [
          {
            capability_id: "skill:cccc:install",
            name: "install",
            description_short: "Install capability",
          },
        ],
        dynamic_tools: [],
      },
    });

    expect(commands.map((item) => item.command)).toEqual(["/install"]);
    expect(commands[0]).toMatchObject({
      command: "/install",
      capabilityId: "skill:cccc:install",
      sourceType: "builtin_command",
    });
  });

  it("keeps skill execution separate from dynamic tool payloads", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        dynamic_tools: [
          {
            capability_id: "mcp:docs",
            name: "search_docs",
            real_tool_name: "search_docs",
            inputSchema: { properties: { query: { type: "string" } }, required: ["query"] },
          },
        ],
      },
    });
    const tool = parseSlashCommandInput("/search_docs capability use", commands);

    expect(parseSlashCommandInput("/writer make this concise", commands)).toBeNull();
    expect(tool?.item.sourceType).toBe("dynamic_tool");
    expect(buildSlashCommandToolArgumentsForItem(tool!.item, tool!.argsText)).toEqual({ query: "capability use" });
  });

  it("excludes dynamic tools that cannot be called from a single text argument", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        dynamic_tools: [
          {
            capability_id: "mcp:deploy",
            name: "deploy_app",
            real_tool_name: "deploy_app",
            inputSchema: {
              properties: {
                app: { type: "string" },
                environment: { type: "string" },
              },
              required: ["app", "environment"],
              additionalProperties: false,
            },
          },
          {
            capability_id: "mcp:docs",
            name: "search_docs",
            real_tool_name: "search_docs",
            inputSchema: { properties: { query: { type: "string" } }, required: ["query"] },
          },
        ],
      },
    });

    expect(commands.map((item) => item.command)).toContain("/search_docs");
    expect(commands.map((item) => item.command)).not.toContain("/deploy_app");
    expect(parseSlashCommandInput("/deploy_app web prod", commands)).toBeNull();
  });

  it("does not convert capsule skill arguments into tool arguments", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [{ capability_id: "skill:agent_self_proposed:writer", name: "writer" }],
        dynamic_tools: [],
      },
    });
    const parsed = parseSlashCommandInput("/writer make this concise", commands);

    expect(parsed?.item.sourceType).toBe("capsule_skill");
    expect(parsed?.argsText).toBe("make this concise");
    expect(buildSlashCommandToolArgumentsForItem(parsed!.item, parsed!.argsText)).toEqual({});
  });

  it("builds a chat dispatch message for capsule skill commands with arguments", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [{ capability_id: "skill:agent_self_proposed:writer", name: "writer" }],
        dynamic_tools: [],
      },
    });
    const parsed = parseSlashCommandInput("/writer make this concise", commands);

    expect(buildCapsuleSkillDispatchText(parsed!.item, parsed!.argsText)).toBe(
      "请使用已激活的 /writer skill 完成以下任务：\n\nmake this concise",
    );
    expect(buildCapsuleSkillDispatchText(parsed!.item, "")).toBe("");
  });

  it("requires task text for capsule skill slash commands instead of activating the skill again", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [{ capability_id: "skill:agent_self_proposed:writer", name: "writer" }],
        dynamic_tools: [],
      },
    });
    const parsed = parseSlashCommandInput("/writer", commands);

    expect(resolveCapsuleSkillSlashCommand(parsed!.item, parsed!.argsText, {
      missingArgs: (command) => `请在 ${command} 后输入任务。`,
    })).toEqual({
      kind: "missing_args",
      message: "请在 /writer 后输入任务。",
    });
    expect(resolveCapsuleSkillSlashCommand(parsed!.item, "make this concise")).toEqual({
      kind: "dispatch",
      dispatchText: "请使用已激活的 /writer skill 完成以下任务：\n\nmake this concise",
    });
  });

  it("keeps slash command execution guards as a pure decision", () => {
    const messages = {
      attachmentsUnsupported: "附件不能用于斜杠命令。",
      repliesUnsupported: "不能回复某条消息。",
      quotedPresentationUnsupported: "引用演示不能用于斜杠命令。",
      crossGroupUnsupported: "跨工作组发送不能用于斜杠命令。",
    };

    expect(resolveSlashCommandGuard({
      composerFilesCount: 1,
      hasReplyTarget: false,
      hasQuotedPresentationRef: false,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    }, messages)).toEqual({ ok: false, message: "附件不能用于斜杠命令。" });

    expect(resolveSlashCommandGuard({
      composerFilesCount: 0,
      hasReplyTarget: true,
      sourceType: "dynamic_tool",
      hasQuotedPresentationRef: false,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    }, messages)).toEqual({ ok: false, message: "不能回复某条消息。" });

    expect(resolveSlashCommandGuard({
      composerFilesCount: 0,
      hasReplyTarget: false,
      hasQuotedPresentationRef: true,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    }, messages)).toEqual({ ok: false, message: "引用演示不能用于斜杠命令。" });

    expect(resolveSlashCommandGuard({
      composerFilesCount: 0,
      hasReplyTarget: false,
      hasQuotedPresentationRef: false,
      sendGroupId: "g2",
      selectedGroupId: "g1",
    }, messages)).toEqual({ ok: false, message: "跨工作组发送不能用于斜杠命令。" });

    expect(resolveSlashCommandGuard({
      composerFilesCount: 0,
      hasReplyTarget: false,
      hasQuotedPresentationRef: false,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    })).toEqual({ ok: true });
  });

  it("allows reply targets for message-backed slash commands only", () => {
    const base = {
      composerFilesCount: 0,
      hasReplyTarget: true,
      hasQuotedPresentationRef: false,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    };

    expect(resolveSlashCommandGuard({ ...base, sourceType: "builtin_command" })).toEqual({ ok: true });
    expect(resolveSlashCommandGuard({ ...base, sourceType: "capsule_skill" })).toEqual({ ok: true });
    expect(resolveSlashCommandGuard({ ...base, sourceType: "dynamic_tool" })).toEqual({
      ok: false,
      message: "Slash command does not support replying to a specific message yet.",
    });
  });

  it("allows slash commands when the message only requires recipients to reply", () => {
    expect(resolveSlashCommandGuard({
      composerFilesCount: 0,
      hasReplyTarget: false,
      replyRequired: true,
      hasQuotedPresentationRef: false,
      sendGroupId: "g1",
      selectedGroupId: "g1",
    })).toEqual({ ok: true });
  });

  it("shows every available slash command for an empty slash query", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: Array.from({ length: 10 }, (_, index) => ({
          capability_id: `skill:agent_self_proposed:skill-${index}`,
          name: `skill-${index}`,
        })),
        dynamic_tools: [],
      },
    });

    expect(commands).toHaveLength(11);
    expect(filterSlashCommands(commands, "/")).toHaveLength(11);
  });

  it("paginates visible slash commands without changing the cached command list", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: Array.from({ length: 10 }, (_, index) => ({
          capability_id: `skill:agent_self_proposed:skill-${index}`,
          name: `skill-${index}`,
        })),
        dynamic_tools: [],
      },
    });

    expect(getVisibleSlashCommandPage(commands, 8)).toHaveLength(8);
    expect(getVisibleSlashCommandPage(commands, 16)).toHaveLength(11);
    expect(commands).toHaveLength(11);
  });

  it("only parses slash commands at the start of composer text", () => {
    const commands = buildSlashCommands({
      state: {
        group_id: "g1",
        actor_id: "user",
        enabled: [],
        active_capsule_skills: [{ capability_id: "skill:agent_self_proposed:writer", name: "writer" }],
        dynamic_tools: [],
      },
    });

    expect(parseSlashCommandInput("/writer", commands)?.item.command).toBe("/writer");
    expect(parseSlashCommandInput("please /writer", commands)).toBeNull();
    expect(parseSlashCommandInput(" /writer", commands)).toBeNull();
    expect(filterSlashCommands(commands, "http://example.test/path")).toEqual([]);
  });
});
