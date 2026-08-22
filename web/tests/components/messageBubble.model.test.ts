import { describe, expect, it } from "vite-plus/test";

import {
  buildToLabel,
  buildVisibleReadStatusEntries,
  computeObligationSummary,
  getSenderDisplayName,
} from "../../src/components/messageBubble/model";

function typedFixture<T>(value: unknown): T {
  return value as unknown as T;
}

describe("messageBubble model", () => {
  it("shows remote recipients for cross-group source records", () => {
    expect(
      buildToLabel({
        hasDestination: true,
        dstGroupId: "g-2",
        dstTo: ["@foreman", "alice"],
        groupLabelById: { "g-2": "第二组" },
        recipients: ["ignored"],
        displayNameMap: new Map([["alice", "Alice"]]),
      }),
    ).toBe("@foreman, Alice");
  });

  it("builds recipient label from display names for local messages", () => {
    expect(
      buildToLabel({
        hasDestination: false,
        dstGroupId: "",
        dstTo: [],
        groupLabelById: {},
        recipients: ["alice", "bob"],
        displayNameMap: new Map([
          ["alice", "Alice"],
          ["bob", "Bob"],
        ]),
      }),
    ).toBe("Alice, Bob");
  });

  it("prefers actor title when computing sender display name", () => {
    expect(
      getSenderDisplayName(
        typedFixture<Parameters<typeof getSenderDisplayName>[0]>({
          senderId: "architect",
          senderActor: { id: "architect", title: "架构设计专家" },
          displayNameMap: new Map([["architect", "Architect"]]),
        }),
      ),
    ).toBe("架构设计专家");
  });

  it("uses Group Bridge source name instead of peer id for remote messages", () => {
    expect(
      getSenderDisplayName({
        senderId: "group_bridge:12D3KooWAXEk8Zw3BMLku6AGNrctVsC9beZsAAEttE798N5HYf1a",
        senderActor: null,
        senderTitle: "",
        group_bridgeSourceName: "CCCC Cross Test",
        displayNameMap: new Map(),
      }),
    ).toBe("CCCC Cross Test");
  });

  it("keeps only actors present in read status", () => {
    expect(
      buildVisibleReadStatusEntries(
        typedFixture<Parameters<typeof buildVisibleReadStatusEntries>[0]>([
          { id: "a-1" },
          { id: "a-2" },
          { id: "a-3" },
        ]),
        typedFixture<Parameters<typeof buildVisibleReadStatusEntries>[1]>({
          "a-1": true,
          "a-3": false,
        }),
      ),
    ).toEqual([
      ["a-1", true],
      ["a-3", false],
    ]);
  });

  it("computes reply obligation summary for requested recipients", () => {
    expect(
      computeObligationSummary(
        typedFixture<Parameters<typeof computeObligationSummary>[0]>({
          hideDirectUserObligationSummary: false,
          obligationStatus: {
            alice: { reply_requested: true, replied: true, cancelled: false },
            bob: { reply_requested: true, replied: false, cancelled: false },
          },
        }),
      ),
    ).toEqual({ done: 1, total: 2 });
  });

  it("ignores status entries without a reply request", () => {
    expect(
      computeObligationSummary(
        typedFixture<Parameters<typeof computeObligationSummary>[0]>({
          hideDirectUserObligationSummary: false,
          obligationStatus: { alice: { reply_requested: false, replied: true, cancelled: false } },
        }),
      ),
    ).toBeNull();
  });
});
