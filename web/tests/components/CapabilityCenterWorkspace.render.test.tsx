import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";

import { CapabilityCenterWorkspace } from "../../src/components/capabilities/CapabilityCenterWorkspace";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("../../src/services/api", () => ({
  fetchCapabilityOverview: vi.fn(),
  fetchGroupCapabilityState: vi.fn(),
  updateGroupCapabilityVisibility: vi.fn(),
  enableGroupCapability: vi.fn(),
  uninstallCapability: vi.fn(),
  blockCapabilityGlobal: vi.fn(),
  updateCapabilityAllowlist: vi.fn(),
  deleteCapabilitySource: vi.fn(),
}));

describe("CapabilityCenterWorkspace rendering", () => {
  it("keeps the empty details rail vertical-only", () => {
    const html = renderToStaticMarkup(<CapabilityCenterWorkspace isOpen groupId="g-demo" surface="page" />);

    expect(html).toContain("overflow-x-hidden");
    expect(html).toContain("overflow-y-auto");
  });

  it("wraps long details text instead of allowing horizontal overflow", () => {
    const source = readFileSync(resolve(process.cwd(), "src/components/capabilities/CapabilityCenterWorkspace.tsx"), "utf8");

    expect(source).toContain("[overflow-wrap:anywhere]");
    expect(source).toContain("break-words");
  });

  it("uses mobile cards and scrollable shadcn-style controls instead of a forced table", () => {
    const source = readFileSync(resolve(process.cwd(), "src/components/capabilities/CapabilityCenterWorkspace.tsx"), "utf8");

    expect(source).toContain("md:hidden");
    expect(source).toContain("overflow-x-auto");
    expect(source).toContain("[scrollbar-width:none]");
    expect(source).toContain("CapabilityRowActions");
    expect(source).toContain("h-10 w-10");
  });

  it("keeps mobile chrome compact so capability cards reach the first viewport", () => {
    const source = readFileSync(resolve(process.cwd(), "src/components/capabilities/CapabilityCenterWorkspace.tsx"), "utf8");

    expect(source).toContain("hidden grid-cols-2 gap-2 sm:grid");
    expect(source).toContain("hidden truncate text-xs");
    expect(source).toContain("hidden items-center gap-2 sm:flex");
    expect(source).toContain("mobileControlsOpen");
    expect(source).toContain("aria-expanded={mobileControlsOpen}");
    expect(source).toContain("min-h-[56px]");
    expect(source).not.toContain("grid grid-cols-2 gap-2 sm:grid-cols-4");
  });
});
