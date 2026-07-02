import { beforeEach, describe, expect, it, vi } from "vitest";

const apiMocks = vi.hoisted(() => ({
  sendMessage: vi.fn(),
  replyMessage: vi.fn(),
  dispatchSlashSkill: vi.fn(),
}));

vi.mock("../../src/services/api", () => apiMocks);

import { sendSlashSkillMessageRequest } from "../../src/hooks/useSlashSkillDispatch";

describe("sendSlashSkillMessageRequest", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("sends builtin slash commands through the normal message API", async () => {
    apiMocks.sendMessage.mockResolvedValueOnce({ ok: true, result: {} });

    await expect(sendSlashSkillMessageRequest({
      selectedGroupId: "g1",
      message: "/install cccc",
      command: "",
      capabilityId: "",
      toTokens: ["@all"],
      priority: "normal",
      replyRequired: false,
      localId: "local-builtin",
      replyTarget: null,
    })).resolves.toEqual({ ok: true, result: {} });

    expect(apiMocks.sendMessage).toHaveBeenCalledWith(
      "g1",
      "/install cccc",
      ["@all"],
      undefined,
      "normal",
      false,
      "local-builtin",
      [],
    );
    expect(apiMocks.dispatchSlashSkill).not.toHaveBeenCalled();
    expect(apiMocks.replyMessage).not.toHaveBeenCalled();
  });

  it("uses the reply API when slash skill dispatch has a reply target", async () => {
    apiMocks.dispatchSlashSkill.mockResolvedValueOnce({ ok: true, result: {} });

    await expect(sendSlashSkillMessageRequest({
      selectedGroupId: "g1",
      message: "开始执行",
      command: "/using-superpowers",
      capabilityId: "skill:agent_self_proposed:using-superpowers",
      toTokens: ["@all"],
      priority: "attention",
      replyRequired: true,
      localId: "local-1",
      replyTarget: {
        eventId: "evt-original",
        by: "foreman",
        text: "失败日志",
      },
    })).resolves.toEqual({ ok: true, result: {} });

    expect(apiMocks.dispatchSlashSkill).toHaveBeenCalledWith(
      "g1",
      {
        taskText: "开始执行",
        command: "/using-superpowers",
        capabilityId: "skill:agent_self_proposed:using-superpowers",
        to: ["@all"],
        priority: "attention",
        replyRequired: true,
        clientId: "local-1",
        replyTo: "evt-original",
        quoteText: "失败日志",
      },
    );
    expect(apiMocks.replyMessage).not.toHaveBeenCalled();
    expect(apiMocks.sendMessage).not.toHaveBeenCalled();
  });

  it("uses the hidden slash skill dispatch API when there is no reply target", async () => {
    apiMocks.dispatchSlashSkill.mockResolvedValueOnce({ ok: true, result: {} });

    await expect(sendSlashSkillMessageRequest({
      selectedGroupId: "g1",
      message: "开始执行",
      command: "/using-superpowers",
      capabilityId: "skill:agent_self_proposed:using-superpowers",
      toTokens: ["@all"],
      priority: "normal",
      replyRequired: false,
      localId: "local-2",
      replyTarget: null,
    })).resolves.toEqual({ ok: true, result: {} });

    expect(apiMocks.dispatchSlashSkill).toHaveBeenCalledWith(
      "g1",
      {
        taskText: "开始执行",
        command: "/using-superpowers",
        capabilityId: "skill:agent_self_proposed:using-superpowers",
        to: ["@all"],
        priority: "normal",
        replyRequired: false,
        clientId: "local-2",
        replyTo: "",
        quoteText: "",
      },
    );
    expect(apiMocks.sendMessage).not.toHaveBeenCalled();
    expect(apiMocks.replyMessage).not.toHaveBeenCalled();
  });
});
