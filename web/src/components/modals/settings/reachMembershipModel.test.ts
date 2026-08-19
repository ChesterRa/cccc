import { describe, expect, it } from "vite-plus/test";

import {
  hostnameLooksTokenless,
  membershipCopyRows,
  membershipPanelKind,
  type MembershipState,
} from "./reachMembershipModel";

function membership(partial: Partial<MembershipState>): MembershipState {
  return { logged_in: false, ...partial };
}

describe("reach membership copy rows", () => {
  it("hides all three strings until the machine is linked", () => {
    expect(
      membershipCopyRows(membership({ logged_in: false, hostname: "https://d-1.cccc.foo" })),
    ).toEqual([]);
  });

  it("keeps hostname, web, and connector as separate rows", () => {
    const rows = membershipCopyRows(
      membership({
        logged_in: true,
        hostname: "https://d-1.cccc.foo",
        web_url: "https://d-1.cccc.foo/ui/?token=acc_secret",
        connector_url: "https://d-1.cccc.foo/mcp/web-model/wmc_1/token/secret",
      }),
    );
    expect(rows.map((row) => row.id)).toEqual(["hostname", "web", "connector"]);
    expect(rows[0]?.value).toBe("https://d-1.cccc.foo");
    expect(rows[1]?.value).toContain("token=acc_secret");
    expect(rows[2]?.value).toContain("/token/secret");
  });

  it("marks a missing connector as unavailable instead of inventing a URL", () => {
    const rows = membershipCopyRows(
      membership({
        logged_in: true,
        hostname: "https://d-1.cccc.foo",
        web_url: "https://d-1.cccc.foo/ui/?token=acc_secret",
      }),
    );
    expect(rows.find((row) => row.id === "connector")).toEqual({
      id: "connector",
      value: "",
      available: false,
    });
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
  it("splits logged out, cut, offline, and online", () => {
    expect(membershipPanelKind(null)).toBe("logged_out");
    expect(membershipPanelKind(membership({ logged_in: true, cut: true }))).toBe("cut");
    expect(membershipPanelKind(membership({ logged_in: true, online: false }))).toBe("offline");
    expect(membershipPanelKind(membership({ logged_in: true, online: true }))).toBe("online");
  });
});
