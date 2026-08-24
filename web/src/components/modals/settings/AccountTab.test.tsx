import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { MembershipState } from "../../../types";
import { AccountTab } from "./AccountTab";

const state = vi.hoisted(() => ({
  membership: { logged_in: false } as MembershipState | null,
  busy: false,
  error: "",
}));
const action = vi.hoisted(() => vi.fn(async () => true));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en-US" },
  }),
}));
vi.mock("./useMembershipController", () => ({
  useMembershipController: () => ({
    membership: state.membership,
    membershipBusy: state.busy,
    membershipError: state.error,
    membershipPollReady: true,
    reachBusy: false,
    reachAction: null,
    refresh: action,
    connect: action,
    poll: action,
    disconnect: action,
    startReach: action,
    stopReach: action,
  }),
}));

describe("AccountTab", () => {
  beforeEach(() => {
    state.membership = { logged_in: false };
    state.busy = false;
    state.error = "";
    action.mockClear();
  });

  it("does not present a link action before the initial account check completes", () => {
    state.membership = null;
    state.busy = true;

    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.status.loading");
    expect(html).not.toContain("account.linkInstallation");
  });

  it("keeps a failed initial check distinct from an unlinked installation", () => {
    state.membership = null;
    state.error = "account service unavailable";

    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.status.unavailable");
    expect(html).toContain("account service unavailable");
    expect(html).not.toContain("account.linkInstallation");
  });

  it("presents account linking as optional for local CCCC", () => {
    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.linkInstallation");
    expect(html).toContain("account.stateHelp.logged_out");
    expect(html).toContain("account.servicesNeedLink");
  });

  it("shows the bound issuer and installation without exposing a Web bearer URL", () => {
    state.membership = {
      logged_in: true,
      device_id: "device-1",
      account_origin: "https://account.example.test/",
      web_url: "https://reach.example.test/ui/?token=secret",
      online: false,
    };

    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("device-1");
    expect(html).toContain("https://account.example.test/");
    expect(html).toContain("account.manageAccount");
    expect(html).not.toContain("reach.example.test");
    expect(html).not.toContain("token=secret");
  });

  it("keeps transient account unavailability distinct from a cut device", () => {
    state.membership = {
      logged_in: true,
      device_id: "device-1",
      account_origin: "https://account.example.test/",
      account_reachable: false,
      cut: false,
    };

    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.accountUnavailable");
    expect(html).toContain("account.status.offline");
    expect(html).not.toContain("account.relinkInstallation");
  });

  it("explains unsupported Reach without hiding the linked account", () => {
    state.membership = {
      logged_in: true,
      device_id: "device-1",
      account_origin: "https://account.example.test/",
      reach_supported: false,
    };

    const html = renderToStaticMarkup(
      <AccountTab isDark={false} onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.status.offline");
    expect(html).toContain("account.reachUnsupported");
  });

  it("preserves the return intent when Account was opened from Web Access", () => {
    const html = renderToStaticMarkup(
      <AccountTab isDark={false} returnToWebAccess onOpenWebAccess={() => undefined} />,
    );

    expect(html).toContain("account.continueWebAccess");
    expect(html).not.toContain(">account.openWebAccess<");
  });
});
