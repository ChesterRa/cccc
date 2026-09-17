// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { apiJson } from "../services/api/base";
import { loadConnectMentionGroups, type ConnectMentionGroup } from "./useConnectMentionGroups";
import { buildComposerMentionSuggestions } from "../pages/chat/chatMentionSuggestions";
import {
  createComposerGroupMentionToken,
  resolveControlledComposerMentionContext,
} from "./composerGroupMentions";
import {
  buildComposerConnectGroupRefs,
  buildComposerLocalGroupRouteRefs,
} from "./composerLocalGroupRouteRefs";
import { buildComposerSendPlanTargets } from "./composerSendPlan";
import { useComposerStore } from "../stores/useComposerStore";
import { useGroupStore } from "../stores/useGroupStore";
import { restoreFailedSendComposerState } from "./chat/chatComposerState";
import type { GroupMeta } from "../types";

vi.mock("../services/api/base", async (original) => ({ ...(await original()), apiJson: vi.fn() }));
const remote: ConnectMentionGroup = {
  instance_id: "i_mac",
  instance_name: "Mac",
  group_id: "g_same",
  title: "Team",
  actors: [{ id: "worker", title: "Worker", enabled: true }],
  fresh: true,
};
const local = [{ group_id: "g_same", title: "Team" }] as GroupMeta[];
const token = createComposerGroupMentionToken({
  groupId: remote.group_id,
  remote,
  token: "#Team · Mac",
  start: 0,
})!;
const ok = (result: unknown) => ({ ok: true as const, result });

describe("Connect Group references", () => {
  beforeEach(() => {
    vi.mocked(apiJson).mockReset();
  });

  it("keeps local and multiple remote instances with the same Group ID distinct", () => {
    const suggestions = buildComposerMentionSuggestions({
      kind: "group",
      filter: "Team",
      recipientActors: [],
      groups: local,
      remoteGroups: [remote, { ...remote, instance_id: "i_linux", instance_name: "Linux" }],
    });
    expect(suggestions.map((s) => s.label)).toEqual(["Team", "Team · Mac", "Team · Linux"]);
    expect(
      buildComposerMentionSuggestions({
        kind: "group",
        filter: "Mac",
        recipientActors: [],
        groups: local,
        remoteGroups: [remote],
      }),
    ).toHaveLength(1);
  });

  it("sends only locally and carries the exact selected remote identity as context", () => {
    const text = "#Team · Mac please ask for help";
    const args = { text, selectedGroupId: "g_same", groups: local, tokens: [token] };
    expect(buildComposerLocalGroupRouteRefs(args)).toEqual([]);
    expect(buildComposerConnectGroupRefs(text, [token])).toEqual([
      {
        kind: "connect_group_ref",
        instance_id: "i_mac",
        instance_name: "Mac",
        group_id: "g_same",
        group_title: "Team",
        token: "#Team · Mac",
      },
    ]);
    expect(
      buildComposerSendPlanTargets({
        ...args,
        groupMentionTokens: [token],
        dstGroupId: "g_same",
        isCrossGroup: false,
      }),
    ).toEqual([{ groupId: "g_same", isCrossGroup: false, source: "selected_group" }]);
    expect(buildComposerConnectGroupRefs(text, [])).toEqual([]);
    expect(buildComposerConnectGroupRefs(text.replace("Mac", "Other"), [token])).toEqual([]);
    expect(buildComposerConnectGroupRefs("", [token])).toEqual([]);
  });

  it("never looks up remote Actor hints using a local Group ID", () => {
    const text = "#Team · Mac @";
    expect(
      resolveControlledComposerMentionContext({ text, atIndex: text.length - 1, tokens: [token] }),
    ).toEqual({ scope: "destination", mentionTargetGroupId: "", remote });
    expect(
      resolveControlledComposerMentionContext({
        text: "#Team · Mac\n@",
        atIndex: 12,
        tokens: [token],
      }).remote,
    ).toBeUndefined();
  });

  it("preserves the identity through Group switching and clears it with the draft", () => {
    const state = useComposerStore.getState();
    state.clearComposer();
    useComposerStore.setState({
      activeGroupId: "a",
      composerText: "#Team · Mac",
      composerGroupMentionTokens: [token],
      drafts: {},
    });
    state.switchGroup("a", "b");
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([]);
    state.switchGroup("b", "a");
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([token]);
    state.clearComposer();
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([]);
  });

  it("restores failed sends to their own draft without attaching refs to the newly selected Group", () => {
    useComposerStore.setState({
      activeGroupId: "b",
      composerText: "new draft",
      composerGroupMentionTokens: [],
      drafts: {},
    });
    useGroupStore.setState({ selectedGroupId: "b" });
    restoreFailedSendComposerState({
      originGroupId: "a",
      composerText: "#Team · Mac",
      composerGroupMentionTokens: [token],
      composerFiles: [],
      toText: "",
      replyTarget: null,
      quotedPresentationRef: null,
      quotedVoiceDocumentRef: null,
      messageMode: "send",
    });
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([]);
    useComposerStore.getState().switchGroup("b", "a");
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([token]);
  });

  it("reads all same-account pages and exact external Groups, including Direct without Web access", async () => {
    const queried: URLSearchParams[] = [];
    vi.mocked(apiJson).mockImplementation(async (path) => {
      const q = new URL(String(path), "https://fixture.test").searchParams;
      expect(String(path)).toContain("/groups/g_source/connect/catalog");
      queried.push(q);
      if (!q.has("instance_id"))
        return ok({
          instances: [{ instance_id: "i_mac", display_name: "Mac" }],
          external_groups: [
            {
              instance: { instance_id: "i_direct", display_name: "Direct" },
              group_id: "g_only",
              title: "Shared",
            },
          ],
        });
      if (q.get("instance_id") === "i_direct") {
        expect(q.get("target_group_id")).toBe("g_only");
        return ok({
          instance: { instance_id: "i_direct", display_name: "Direct" },
          catalog: { groups: [{ ...remote, group_id: "g_only" }] },
          fresh: false,
          next: null,
        });
      }
      return ok({
        instance: { instance_id: "i_mac", display_name: "Mac" },
        catalog: { groups: [{ ...remote, group_id: q.has("after") ? "g_z" : "g_a" }] },
        fresh: true,
        next: q.has("after") ? null : "g_a",
      });
    });
    const result = await loadConnectMentionGroups("g_source", new AbortController().signal);
    expect(result.incomplete).toBe(false);
    expect(result.groups.map((g) => g.group_id).sort()).toEqual(["g_a", "g_only", "g_z"]);
    expect(result.groups.find((g) => g.instance_id === "i_direct")?.fresh).toBe(false);
    expect(queried).toHaveLength(4);
  });

  it("keeps restricted views local and reports unavailable catalogs instead of confirmed emptiness", async () => {
    vi.mocked(apiJson).mockResolvedValue({
      ok: false,
      error: { code: "admin_required", message: "administrator access required" },
    });
    expect(await loadConnectMentionGroups("g", new AbortController().signal)).toEqual({
      groups: [],
      incomplete: false,
    });
    vi.mocked(apiJson).mockResolvedValue({
      ok: false,
      error: { code: "io_error", message: "unavailable" },
    });
    expect(await loadConnectMentionGroups("g", new AbortController().signal)).toEqual({
      groups: [],
      incomplete: true,
    });
  });

  it("can reference an authorized external Group before its first Actor catalog, but never a revoked one", async () => {
    vi.mocked(apiJson)
      .mockResolvedValueOnce(
        ok({
          instances: [],
          external_groups: [
            {
              instance: { instance_id: "i_direct", display_name: "Direct" },
              group_id: "g_known",
              title: "Known",
            },
            {
              instance: { instance_id: "i_retired", display_name: "Retired" },
              group_id: "g_revoked",
              title: "Revoked",
            },
          ],
        }),
      )
      .mockResolvedValueOnce(
        ok({
          instance: { instance_id: "i_direct", display_name: "Direct" },
          catalog: null,
          fresh: false,
          next: null,
        }),
      )
      .mockResolvedValueOnce({
        ok: false,
        error: { code: "connect_peer_unavailable", message: "retired" },
      });
    const result = await loadConnectMentionGroups("g", new AbortController().signal);
    expect(result.groups).toEqual([
      {
        instance_id: "i_direct",
        instance_name: "Direct",
        group_id: "g_known",
        title: "Known",
        actors: [],
        fresh: false,
      },
    ]);
    expect(result.incomplete).toBe(true);
  });
});
