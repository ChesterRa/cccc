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
  fetchGroup: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

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
  vi.mocked(api.fetchGroup).mockResolvedValue({
    ok: true,
    result: { group: { group_id: "g-demo", title: "Demo" } },
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
  // The page opens on the recommendations; this old result is listed under Skills.
  const skills = [...host.querySelectorAll("button")].find((button) =>
    button.textContent?.includes("capabilityCenter.sections.skill.label"),
  );
  await act(async () => skills?.click());

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

it("shows server matches from use cases after the search response arrives", async () => {
  const checklist = {
    capability_id: "skill:test:quality",
    kind: "skill",
    name: "Delivery checklist",
    description_short: "Check an analysis before sharing it",
    source_id: "manual_import",
    use_when: ["Review data and validation results before delivery"],
    qualification_status: "qualified",
    enable_supported: true,
  };
  vi.mocked(api.fetchCapabilityOverview).mockImplementation(
    async ({ query } = {}) =>
      ({
        ok: true,
        result: { items: query ? [checklist] : [legacy], total_count: 1, has_more: false },
      }) as never,
  );
  await act(async () => root.render(<CapabilityCenterWorkspace isOpen groupId="g-demo" />));
  const skills = [...host.querySelectorAll("button")].find((button) =>
    button.textContent?.includes("capabilityCenter.sections.skill.label"),
  );
  await act(async () => skills?.click());
  const input = host.querySelector<HTMLInputElement>(
    'input[placeholder="capabilityCenter.searchPlaceholder"]',
  )!;

  vi.useFakeTimers();
  try {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
        input,
        "data validation",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    // Until the server answers, typing still filters the previous list immediately.
    expect(host.textContent).not.toContain("Old search result");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(api.fetchCapabilityOverview).toHaveBeenLastCalledWith(
      expect.objectContaining({ query: "data validation" }),
    );
    // The words are in use_when and are not adjacent; the narrow local matcher
    // must not hide the server's successful result in either list layout.
    expect(host.textContent).toContain("Delivery checklist");
    expect(host.querySelectorAll('[role="button"]')).not.toHaveLength(0);
  } finally {
    vi.useRealTimers();
  }
});
