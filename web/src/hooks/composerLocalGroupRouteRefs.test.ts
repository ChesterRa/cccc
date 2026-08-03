import { describe, expect, it } from "vite-plus/test";

import type { GroupMeta } from "../types";
import { createComposerGroupMentionToken } from "./composerGroupMentions";
import { buildComposerLocalGroupRouteRefs } from "./composerLocalGroupRouteRefs";

const groups = [
  { group_id: "g_local", title: "Local" },
  { group_id: "self-agent", title: "Self Agent" },
] as GroupMeta[];

describe("buildComposerLocalGroupRouteRefs", () => {
  it("builds context for a menu-selected local group without creating a send", () => {
    const text = "请 #Self Agent 主动打个招呼";
    const token = createComposerGroupMentionToken({
      groupId: "self-agent",
      token: "#Self Agent",
      start: text.indexOf("#Self Agent"),
    })!;

    expect(
      buildComposerLocalGroupRouteRefs({
        text,
        selectedGroupId: "g_local",
        tokens: [token],
        groups,
      }),
    ).toEqual([
      {
        kind: "local_group_route",
        group_id: "self-agent",
        group_title: "Self Agent",
        token: "#Self Agent",
      },
    ]);
  });

  it("ignores copied text and remote Group Bridge selections", () => {
    expect(
      buildComposerLocalGroupRouteRefs({
        text: "复制 #Self Agent 不算选择",
        selectedGroupId: "g_local",
        tokens: [],
        groups,
      }),
    ).toEqual([]);

    const text = "请 #Remote Product 主动联系";
    const token = createComposerGroupMentionToken({
      groupId: "g_remote",
      token: "#Remote Product",
      start: text.indexOf("#Remote Product"),
    })!;
    expect(
      buildComposerLocalGroupRouteRefs({
        text,
        selectedGroupId: "g_local",
        tokens: [token],
        groups: [
          ...groups,
          { group_id: "g_remote", title: "Remote Product", group_bridge_remote: true },
        ],
      }),
    ).toEqual([]);
  });
});
