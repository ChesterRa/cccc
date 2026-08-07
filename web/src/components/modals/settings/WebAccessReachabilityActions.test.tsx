// @vitest-environment happy-dom
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { WebAccessReachabilityActions } from "./WebAccessReachabilityActions";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const callbacks = { onSave: vi.fn(), onApply: vi.fn(), onCopyEndpoint: vi.fn() };

function renderActions(
  overrides: Partial<Parameters<typeof WebAccessReachabilityActions>[0]> = {},
): { buttons: HTMLButtonElement[]; container: HTMLDivElement } {
  const markup = renderToStaticMarkup(
    <WebAccessReachabilityActions
      action="apply"
      actionHint="ready"
      draftGoal="lan"
      savedGoal="lan"
      hasAdminToken={false}
      saveBusy={false}
      applyBusy={false}
      endpoint="http://192.168.1.8:8848/ui/"
      {...callbacks}
      {...overrides}
    />,
  );
  const container = document.createElement("div");
  container.innerHTML = markup;
  return { buttons: [...container.querySelectorAll("button")], container };
}

describe("WebAccessReachabilityActions", () => {
  it("disables apply and endpoint copy for remote access without an admin token", () => {
    const [primary, copy] = renderActions().buttons;

    expect(primary.disabled).toBe(true);
    expect(copy.disabled).toBe(true);
    expect(primary.title).toBe("webAccess.remoteAdminTokenRequiredHint");
  });

  it("uses the draft goal for save while keeping a saved local endpoint copyable", () => {
    const [primary, copy] = renderActions({ action: "save", savedGoal: "local" }).buttons;

    expect(primary.disabled).toBe(true);
    expect(copy.disabled).toBe(false);
  });

  it("keeps remote actions enabled once an admin token exists", () => {
    const [primary, copy] = renderActions({ hasAdminToken: true }).buttons;

    expect(primary.disabled).toBe(false);
    expect(copy.disabled).toBe(false);
  });

  it("does not require a token for local-only apply", () => {
    const [primary, copy] = renderActions({ draftGoal: "local", savedGoal: "local" }).buttons;

    expect(primary.disabled).toBe(false);
    expect(copy.disabled).toBe(false);
  });

  it("stacks on mobile, uses equal tablet columns, and keeps wide-screen actions aligned", () => {
    const { buttons } = renderActions({ hasAdminToken: true });
    const [primary, copy] = buttons;
    const actionGroup = primary.parentElement;

    expect(actionGroup?.className).toContain("grid-cols-1");
    expect(actionGroup?.className).toContain("sm:grid-cols-2");
    expect(actionGroup?.className).toContain("xl:flex-nowrap");
    expect(actionGroup?.className).not.toContain("flex-wrap");
    expect(primary.className).toContain("w-full");
    expect(primary.className).toContain("xl:w-auto");
    expect(copy.className).toContain("xl:whitespace-nowrap");
  });
});
