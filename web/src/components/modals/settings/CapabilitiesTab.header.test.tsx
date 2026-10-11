// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const mocks = vi.hoisted(() => ({ overview: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../../services/api", () => ({
  fetchCapabilityOverview: () => mocks.overview(),
  fetchGroups: async () => ({ ok: true, result: { groups: [] } }),
}));
import { CapabilitiesTab } from "./CapabilitiesTab";

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const governanceSection = () =>
  Array.from(host.querySelectorAll("section")).find((section) =>
    section.textContent?.includes("capabilities.title"),
  )!;

describe("capability governance header", () => {
  it("states the page's purpose once, in the subtitle, with nothing under the header", async () => {
    mocks.overview.mockResolvedValue({ ok: true, result: { items: [] } });
    await act(async () => root.render(<CapabilitiesTab isDark={false} isActive />));
    const section = governanceSection();
    expect(section.textContent).toContain("capabilities.subtitle");
    expect(host.textContent).not.toContain("capabilities.pageGuide");
    // The header is the section's only block: no empty body is left behind.
    expect(section.children).toHaveLength(1);
  });

  it("still reports a failed load under the header", async () => {
    mocks.overview.mockResolvedValue({ ok: false, error: { message: "Load failed" } });
    await act(async () => root.render(<CapabilitiesTab isDark={false} isActive />));
    expect(governanceSection().querySelector('[role="alert"]')?.textContent).toBe("Load failed");
  });
});
