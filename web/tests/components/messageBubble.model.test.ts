import { describe, expect, it } from "vitest";

import {
  buildToLabel,
  buildVisibleReadStatusEntries,
  computeAckSummary,
  computeObligationSummary,
  getSenderDisplayName,
} from "../../src/components/messageBubble/model";

describe("messageBubble model", () => {
  it("hides the generic recipient label for cross-group source records", () => {
    expect(buildToLabel({
      hasDestination: true,
      dstGroupId: "g-2",
      dstTo: ["@foreman", "alice"],
      groupLabelById: { "g-2": "第二组" },
      recipients: ["ignored"],
      displayNameMap: new Map([["alice", "Alice"]]),
    })).toBe("");
  });

  it("builds recipient label from display names for local messages", () => {
    expect(buildToLabel({
      hasDestination: false,
      dstGroupId: "",
      dstTo: [],
      groupLabelById: {},
      recipients: ["alice", "bob"],
      displayNameMap: new Map([["alice", "Alice"], ["bob", "Bob"]]),
    })).toBe("Alice, Bob");
  });

  it("prefers actor title when computing sender display name", () => {
    expect(getSenderDisplayName({
      senderId: "architect",
      senderActor: { id: "architect", title: "架构设计专家" },
      displayNameMap: new Map([["architect", "Architect"]]),
    } as any)).toBe("架构设计专家");
  });

  it("uses federation source name instead of peer id for remote messages", () => {
    expect(getSenderDisplayName({
      senderId: "federation:12D3KooWAXEk8Zw3BMLku6AGNrctVsC9beZsAAEttE798N5HYf1a",
      senderActor: null,
      senderTitle: "",
      federationSourceName: "CCCC Cross Test",
      displayNameMap: new Map(),
    })).toBe("CCCC Cross Test");
  });

  it("keeps only actors present in read status", () => {
    expect(buildVisibleReadStatusEntries(
      [{ id: "a-1" }, { id: "a-2" }, { id: "a-3" }] as any,
      { "a-1": true, "a-3": false } as any,
    )).toEqual([["a-1", true], ["a-3", false]]);
  });

  it("computes ack summary only for attention or reply-required messages", () => {
    expect(computeAckSummary({
      hideDirectUserObligationSummary: false,
      isAttention: true,
      replyRequired: false,
      ackStatus: { user: false, peer: true } as any,
      isUserMessage: false,
    })).toEqual({ done: 1, total: 2, needsUserAck: true });
  });

  it("computes reply obligation summary when any recipient requires a reply", () => {
    expect(computeObligationSummary({
      hideDirectUserObligationSummary: false,
      obligationStatus: {
        alice: { reply_required: true, replied: true },
        bob: { reply_required: false, replied: false },
      } as any,
    })).toEqual({ kind: "reply", done: 1, total: 2 });
  });

  it("computes ack obligation summary when no recipient requires a reply", () => {
    expect(computeObligationSummary({
      hideDirectUserObligationSummary: false,
      obligationStatus: {
        alice: { acked: true },
        bob: { acked: false },
      } as any,
    })).toEqual({ kind: "ack", done: 1, total: 2 });
  });
});
