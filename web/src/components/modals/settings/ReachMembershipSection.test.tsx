import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { ReachMembershipSection } from "./ReachMembershipSection";
import type { MembershipState } from "./reachMembershipModel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

const noop = () => undefined;

function render(
  membership: MembershipState | null,
  options: { membershipError?: string; membershipBusy?: boolean; hasAdminToken?: boolean } = {},
) {
  return renderToStaticMarkup(
    <ReachMembershipSection
      membership={membership}
      membershipBusy={options.membershipBusy ?? false}
      membershipError={options.membershipError ?? ""}
      membershipPollReady={true}
      hasAdminToken={options.hasAdminToken ?? false}
      reachBusy={false}
      reachAction={null}
      onConnectAccount={noop}
      onPollAccount={noop}
      onOpenAccount={noop}
      onCreateAdminToken={noop}
      onCreateWebLogin={async () =>
        "https://reach.example.test/api/v1/web_access/exchange?code=wlg_test"
      }
      onReachOn={noop}
      onReachOff={noop}
      onCopied={noop}
      onCopyFailed={noop}
    />,
  );
}

describe("ReachMembershipSection", () => {
  it("starts account authorization in place instead of routing setup away", () => {
    const html = render({ logged_in: false });

    expect(html).toContain("webAccess.reach.setup");
    expect(html).toContain("webAccess.reach.loggedOut");
    expect(html).not.toContain("webAccess.reach.manageAccount");
    expect(html).not.toContain("webAccess.reach.start");
  });

  it("keeps pending approval and status checking in the same surface", () => {
    const html = render({
      logged_in: false,
      account_origin: "https://account.example/",
      pending: {
        user_code: "ABCD-EFGH",
        verification_uri_complete: "https://account.example/device?user_code=ABCD-EFGH",
        interval: 5,
      },
    });

    expect(html).toContain("ABCD-EFGH");
    expect(html).toContain("account.example/device?user_code=ABCD-EFGH");
    expect(html).toContain("webAccess.reach.openApproval");
    expect(html).toContain("webAccess.reach.checkAgain");
  });

  it("shows one tokenless public address and never renders the admin token URL", () => {
    const html = render(
      {
        logged_in: true,
        hostname: "https://d-one.example",
        web_url: "https://d-one.example/ui/",
        online: true,
      },
      { hasAdminToken: true },
    );

    expect(html).toContain("https://d-one.example");
    expect(html).toContain("webAccess.reach.publicAddressLabel");
    expect(html).toContain("webAccess.reach.adminAccessSummary");
    expect(html).toContain("webAccess.reach.copyAdminLink");
    expect(html).not.toContain("token=admin-secret");
    expect(html).not.toContain("webAccess.reach.webLabel");
  });

  it("offers the first missing prerequisite before Reach can start", () => {
    const missingToken = render({ logged_in: true, online: false });
    const ready = render({ logged_in: true, online: false }, { hasAdminToken: true });

    expect(missingToken).toContain("webAccess.reach.createAdminToken");
    expect(missingToken).not.toContain(">webAccess.reach.start</button>");
    expect(ready).toContain(">webAccess.reach.start</button>");
  });

  it("lets an offline Reach owner stop the provider without restarting it first", () => {
    const html = render(
      { logged_in: true, online: false, in_reach: true },
      { hasAdminToken: true },
    );
    const stopButton = html.match(/<button[^>]*>webAccess\.reach\.stop<\/button>/)?.[0];

    expect(stopButton).toBeTruthy();
    expect(stopButton).not.toContain(' disabled=""');
  });

  it("exposes connection failures as an inline alert", () => {
    const html = render(null, { membershipError: "account service unavailable" });

    expect(html).toContain('role="alert"');
    expect(html).toContain("account service unavailable");
  });

  it("does not replay a persisted historical failure as a current alert", () => {
    const html = render(
      { logged_in: true, online: false, last_error: "old Reach binding failure" },
      { hasAdminToken: true },
    );

    expect(html).not.toContain('role="alert"');
    expect(html).not.toContain("old Reach binding failure");
  });

  it("does not misreport the initial status check as logged out", () => {
    const html = render(null, { membershipBusy: true });

    expect(html).toContain("webAccess.reach.statusLoading");
    expect(html).not.toContain("webAccess.reach.loggedOut");
  });

  it("does not offer managed Reach on unsupported platforms", () => {
    const html = render(
      { logged_in: true, online: false, reach_supported: false },
      { hasAdminToken: true },
    );

    expect(html).toContain("webAccess.reach.statusUnsupported");
    expect(html).toContain("webAccess.reach.unsupported");
    expect(html).not.toContain(">webAccess.reach.start</button>");
  });
});
