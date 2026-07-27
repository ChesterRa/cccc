import { describe, expect, it } from "vite-plus/test";

import { buildToLabel, getSenderDisplayName } from "./model";

function labelFor(recipients: string[] | undefined): string {
  return buildToLabel({
    hasDestination: false,
    dstGroupId: "",
    dstTo: [],
    groupLabelById: {},
    recipients,
    displayNameMap: new Map(),
  });
}

describe("buildToLabel", () => {
  it("falls back to @foreman when recipients are empty", () => {
    expect(labelFor([])).toBe("@foreman");
  });

  it("falls back to @foreman when recipients are missing", () => {
    expect(labelFor(undefined)).toBe("@foreman");
  });

  it("keeps explicit @all recipients", () => {
    expect(labelFor(["@all"])).toBe("@all");
  });
});

describe("getSenderDisplayName", () => {
  it("replaces a cross-group sender group id with its display name", () => {
    expect(
      getSenderDisplayName({
        senderId: "g_source::master",
        senderActor: null,
        groupLabelById: { g_source: "wechat-agent" },
        displayNameMap: new Map(),
      }),
    ).toBe("wechat-agent::master");
  });

  it("uses the sender snapshot title for the actor part", () => {
    expect(
      getSenderDisplayName({
        senderId: "g_source::master",
        senderActor: null,
        senderTitle: "主控",
        groupLabelById: { g_source: "wechat-agent" },
        displayNameMap: new Map(),
      }),
    ).toBe("wechat-agent::主控");
  });

  it("keeps the canonical sender id when the group name is unavailable", () => {
    expect(
      getSenderDisplayName({
        senderId: "g_source::master",
        senderActor: null,
        groupLabelById: {},
        displayNameMap: new Map(),
      }),
    ).toBe("g_source::master");
  });
});
