import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { ReachMembershipSection } from "./ReachMembershipSection";
import type { MembershipState } from "./reachMembershipModel";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const noop = () => undefined;

function render(membership: MembershipState | null, membershipError = "", membershipBusy = false) {
  return renderToStaticMarkup(
    <ReachMembershipSection
      membership={membership}
      membershipBusy={membershipBusy}
      membershipError={membershipError}
      reachBusy={false}
      reachAction={null}
      onOpenAccount={noop}
      onReachOn={noop}
      onReachOff={noop}
      onCopied={noop}
      onCopyFailed={noop}
    />,
  );
}

describe("ReachMembershipSection", () => {
  it("keeps the logged-out surface local-first and routes setup to Account", () => {
    const html = render({ logged_in: false });

    expect(html).toContain("webAccess.reach.openAccountSettings");
    expect(html).toContain("webAccess.reach.accountRequired");
    expect(html).not.toContain("webAccess.reach.start");
  });

  it("routes pending approval back to Account instead of duplicating the device flow", () => {
    const html = render({
      logged_in: false,
      pending: {
        user_code: "ABCD-EFGH",
        verification_uri: "https://account.example/device",
        verification_uri_complete: "https://account.example/device?user_code=ABCD-EFGH",
        interval: 5,
      },
    });

    expect(html).toContain("webAccess.reach.accountPending");
    expect(html).toContain("webAccess.reach.openAccountSettings");
    expect(html).not.toContain("ABCD-EFGH");
    expect(html).not.toContain("account.example/device");
  });

  it("shows only global Web credentials and points connectors back to the actor", () => {
    const html = render({
      logged_in: true,
      hostname: "https://d-one.example",
      web_url: "https://d-one.example/ui/?token=admin-secret",
      online: true,
    });

    expect(html).toContain("https://d-one.example");
    expect(html).toContain("token=admin-secret");
    expect(html).toContain("webAccess.reach.connectorManaged");
    expect(html).not.toContain("connector_url");
  });

  it("lets an offline Reach owner stop the provider without restarting it first", () => {
    const html = render({ logged_in: true, online: false, in_reach: true });
    const stopButton = html.match(/<button[^>]*>webAccess\.reach\.stop<\/button>/)?.[0];

    expect(html).toContain("webAccess.reach.openAccountSettings");
    expect(stopButton).toBeTruthy();
    expect(stopButton).not.toContain(' disabled=""');
  });

  it("exposes connection failures as an inline alert", () => {
    const html = render(null, "account service unavailable");

    expect(html).toContain('role="alert"');
    expect(html).toContain("account service unavailable");
  });

  it("does not misreport the initial status check as logged out", () => {
    const html = render(null, "", true);

    expect(html).toContain("webAccess.reach.statusLoading");
    expect(html).not.toContain("webAccess.reach.accountRequired");
  });

  it("disables managed Reach on unsupported platforms", () => {
    const html = render({ logged_in: true, online: false, reach_supported: false });
    const startButton = html.match(/<button[^>]*>webAccess\.reach\.start<\/button>/)?.[0];

    expect(html).toContain("webAccess.reach.statusUnsupported");
    expect(html).toContain("webAccess.reach.unsupported");
    expect(startButton).toContain(' disabled=""');
  });
});
