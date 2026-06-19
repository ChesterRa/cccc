import { describe, expect, it } from "vitest";

import type { GroupMeta } from "../../../types";
import {
  buildRegisterableOptions,
  canSubmitRegister,
  canVerify,
  initialRegisterSelection,
  isPeerHttpRemoteGroupMissing,
  shouldShowRemoteUrlFields,
  registerableOptionsForDisplay,
  safeFederationErrorText,
  toggleGroupSelection,
} from "./federationRegistrationModel";

describe("peer_cccc_http requires remote group id", () => {
  it("flags a missing remote group for peer_cccc_http", () => {
    expect(isPeerHttpRemoteGroupMissing("peer_cccc_http", "")).toBe(true);
    expect(isPeerHttpRemoteGroupMissing("peer_cccc_http", "  ")).toBe(true);
    expect(isPeerHttpRemoteGroupMissing("peer_cccc_http", "g_remote")).toBe(false);
  });

  it("blocks verify and register when peer remote group is empty", () => {
    expect(canVerify({ url: "https://h", transport: "peer_cccc_http", remoteGroupId: "", busy: false })).toBe(false);
    expect(canVerify({ url: "https://h", transport: "peer_cccc_http", remoteGroupId: "g_remote", busy: false })).toBe(true);
    expect(
      canSubmitRegister({ verified: true, url: "https://h", selectedCount: 1, busy: false, transport: "peer_cccc_http", remoteGroupId: "" }),
    ).toBe(false);
    expect(
      canSubmitRegister({ verified: true, url: "https://h", selectedCount: 1, busy: false, transport: "peer_cccc_http", remoteGroupId: "g_remote" }),
    ).toBe(true);
  });
});

describe("federation session pairing mode", () => {
  it("does not show direct Remote URL fields", () => {
    expect(shouldShowRemoteUrlFields("peer_cccc_http")).toBe(true);
    expect(shouldShowRemoteUrlFields("federation_session")).toBe(false);
  });

  it("does not require URL or remote group id for verify/register gating", () => {
    expect(canVerify({ url: "", transport: "federation_session", remoteGroupId: "", busy: false })).toBe(true);
    expect(
      canSubmitRegister({
        verified: true,
        url: "",
        selectedCount: 1,
        busy: false,
        transport: "federation_session",
        remoteGroupId: "",
      }),
    ).toBe(true);
  });
});

const groups = [
  { group_id: "g1", title: "Group One" },
  { group_id: "g2", title: "" },
  { group_id: "", title: "blank" },
] as unknown as GroupMeta[];

describe("federationRegistrationModel", () => {
  it("builds registerable options and drops blank ids", () => {
    const opts = buildRegisterableOptions(groups);
    expect(opts).toEqual([
      { id: "g1", title: "Group One" },
      { id: "g2", title: "g2" },
    ]);
  });

  it("starts with an empty selection (admin not pre-selected)", () => {
    expect(initialRegisterSelection().size).toBe(0);
  });

  it("only shows group checkboxes after a successful verify", () => {
    const opts = buildRegisterableOptions(groups);
    expect(registerableOptionsForDisplay(false, opts)).toEqual([]);
    expect(registerableOptionsForDisplay(true, opts)).toHaveLength(2);
  });

  it("toggles selection immutably", () => {
    const a = toggleGroupSelection(initialRegisterSelection(), "g1");
    expect([...a]).toEqual(["g1"]);
    const b = toggleGroupSelection(a, "g1");
    expect(b.size).toBe(0);
  });

  it("gates register submission on verify + selection", () => {
    expect(canSubmitRegister({ verified: false, url: "https://h", selectedCount: 1, busy: false })).toBe(false);
    expect(canSubmitRegister({ verified: true, url: "", selectedCount: 1, busy: false })).toBe(false);
    expect(canSubmitRegister({ verified: true, url: "https://h", selectedCount: 0, busy: false })).toBe(false);
    expect(canSubmitRegister({ verified: true, url: "https://h", selectedCount: 1, busy: true })).toBe(false);
    expect(
      canSubmitRegister({ verified: true, url: "https://h", selectedCount: 1, busy: false, remoteGroupId: "g_remote" }),
    ).toBe(true);
  });

  it("redacts the user's raw token and acc_-shaped tokens from error text", () => {
    const secret = "acc_deadbeefdeadbeef";
    const text = safeFederationErrorText(`rejected token ${secret} and acc_cafebabecafe`, secret);
    expect(text).not.toContain(secret);
    expect(text).not.toContain("acc_cafebabecafe");
    expect(text).toContain("***");
  });

  it("falls back to a generic message when empty", () => {
    expect(safeFederationErrorText("", "")).toBe("Request failed");
  });
});
