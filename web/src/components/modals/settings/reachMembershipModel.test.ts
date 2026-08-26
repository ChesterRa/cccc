import { describe, expect, it } from "vite-plus/test";

import {
  hostnameLooksTokenless,
  membershipAdminWebUrl,
  membershipApprovalUrl,
  membershipManagementUrl,
  membershipPanelKind,
  membershipPublicAddress,
  type MembershipState,
} from "./reachMembershipModel";

function membership(partial: Partial<MembershipState>): MembershipState {
  return { logged_in: false, ...partial };
}

describe("reach membership addresses", () => {
  it("offers only the tokenless public address while Reach is online", () => {
    const state = membership({
      logged_in: true,
      online: true,
      hostname: "https://d-1.cccc.foo",
      web_url: "https://d-1.cccc.foo/ui/",
    });

    expect(membershipPublicAddress(state)).toBe("https://d-1.cccc.foo");
    expect(membershipAdminWebUrl(state)).toBe("https://d-1.cccc.foo/ui/");
    expect(
      membershipAdminWebUrl(
        membership({ logged_in: true, web_url: "https://d-1.cccc.foo/ui/?token=acc_secret" }),
      ),
    ).toBe("");
  });

  it("does not advertise an offline or credential-bearing hostname", () => {
    expect(
      membershipPublicAddress(
        membership({ logged_in: true, online: false, hostname: "https://d-1.cccc.foo" }),
      ),
    ).toBe("");
    expect(
      membershipPublicAddress(
        membership({
          logged_in: true,
          online: true,
          hostname: "https://d-1.cccc.foo/ui/?token=acc_secret",
        }),
      ),
    ).toBe("");
  });

  it("rejects non-HTTP administrator URLs", () => {
    expect(
      membershipAdminWebUrl(membership({ logged_in: true, web_url: "javascript:alert(1)" })),
    ).toBe("");
  });
});

describe("reach membership hostname safety", () => {
  it("rejects a hostname that already carries a token", () => {
    expect(hostnameLooksTokenless("https://d-1.cccc.foo")).toBe(true);
    expect(hostnameLooksTokenless("https://d-1.cccc.foo/ui/?token=acc_secret")).toBe(false);
    expect(hostnameLooksTokenless("https://d-1.cccc.foo/mcp/web-model/wmc_1/token/secret")).toBe(
      false,
    );
  });
});

describe("reach membership panel kind", () => {
  it("splits logged out, pending, cut, offline, and online", () => {
    expect(membershipPanelKind(null)).toBe("logged_out");
    expect(
      membershipPanelKind(membership({ logged_in: false, pending: { user_code: "ABCD" } })),
    ).toBe("pending");
    expect(membershipPanelKind(membership({ logged_in: true, cut: true }))).toBe("cut");
    expect(membershipPanelKind(membership({ logged_in: true, online: false }))).toBe("offline");
    expect(membershipPanelKind(membership({ logged_in: true, online: true }))).toBe("online");
  });
});

describe("membership account URLs", () => {
  it("accepts only an approval URL on the bound issuer", () => {
    const state = membership({
      account_origin: "https://account.example.test/base",
      pending: {
        user_code: "ABCD",
        verification_uri: "https://account.example.test/device",
        verification_uri_complete: "https://account.example.test/device?user_code=ABCD",
      },
    });
    expect(membershipApprovalUrl(state)).toBe("https://account.example.test/device?user_code=ABCD");
    expect(membershipManagementUrl(state)).toBe("https://account.example.test/base");
    expect(
      membershipApprovalUrl({
        ...state,
        pending: {
          ...state.pending,
          verification_uri_complete: "https://attacker.example.test/device?user_code=ABCD",
        },
      }),
    ).toBe("");
  });

  it("adds the supported UI language without dropping device-code parameters", () => {
    const state = membership({
      account_origin: "https://account.example.test/",
      pending: {
        user_code: "ABCD",
        verification_uri_complete: "https://account.example.test/device?user_code=ABCD",
      },
    });

    expect(membershipApprovalUrl(state, "zh-CN")).toBe(
      "https://account.example.test/device?user_code=ABCD&lang=zh",
    );
    expect(membershipManagementUrl(state, "ja-JP")).toBe("https://account.example.test/?lang=ja");
    expect(membershipManagementUrl(state, "fr-FR")).toBe("https://account.example.test/");
  });
});
