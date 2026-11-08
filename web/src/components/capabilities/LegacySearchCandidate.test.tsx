// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import { CapabilityPicker } from "../CapabilityPicker";
import { CapabilityCenterWorkspace } from "./CapabilityCenterWorkspace";

vi.mock("../../services/api", () => ({
  fetchCapabilityOverview: vi.fn(),
  fetchGroupCapabilityState: vi.fn(),
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const legacy = {
  capability_id: "skill:test:old",
  kind: "skill",
  name: "Old search result",
  source_id: "skillsmp_remote",
  source_uri: "https://example.test/skill",
  qualification_status: "qualified",
  enable_supported: true,
  autoload_candidate: true,
  legacy_search_unverified: true,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.mocked(api.fetchCapabilityOverview).mockResolvedValue({
    ok: true,
    result: { items: [legacy], total_count: 1, has_more: false },
  } as never);
  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue({
    ok: true,
    result: { enabled_capabilities: [legacy.capability_id] },
  } as never);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
});

it("keeps an enabled legacy result manageable, but shows its unverified status", async () => {
  await act(async () => root.render(<CapabilityCenterWorkspace isOpen groupId="g-demo" />));

  expect(host.textContent).toContain("Old search result");
  expect(host.textContent).toContain("capabilityCenter.status.enabled");
  expect(host.textContent).toContain("capabilityCenter.status.legacySearchUnverified");
  expect(host.textContent).not.toContain("qualified");
  expect(host.textContent).not.toContain("capabilityCenter.status.ready");
});

it("marks the old result in the assignment picker without preventing removal", async () => {
  const onChange = vi.fn();
  await act(async () =>
    root.render(
      <CapabilityPicker isDark={false} value={[legacy.capability_id]} onChange={onChange} />,
    ),
  );

  expect(host.textContent).toContain("capabilityCenter.status.legacySearchUnverified");
  const checkbox = host.querySelector<HTMLInputElement>('input[type="checkbox"]');
  expect(checkbox?.checked).toBe(true);
  expect(checkbox?.disabled).toBe(false);
  await act(async () => checkbox?.click());
  expect(onChange).toHaveBeenCalledWith([]);
});
