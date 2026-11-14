// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import type { CapabilityForYou } from "../../types";
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

function release(name: string, availability: CapabilityForYou["availability"]) {
  return {
    capability_id: `mcp:${name}`,
    kind: "mcp_toolpack",
    name,
    content_origin: "release_recommendation",
    source_id: "cccc_recommended",
    qualification_status: "qualified",
    enable_supported: true,
    group_enabled: false,
    members: [
      {
        actor_id: "member-a",
        availability,
        enabled: false,
        blocked: false,
        reason_codes: [],
        enabled_scope: null,
        enabled_scopes: [],
        last_call: null,
      },
    ],
  };
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.mocked(api.fetchCapabilityOverview).mockResolvedValue({
    ok: true,
    result: {
      items: [release("needs-a-setting", "needs_setup"), release("usable", "compatible")],
      total_count: 2,
      has_more: false,
    },
  } as never);
  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue({
    ok: true,
    result: { enabled_capabilities: [] },
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

it("lists a recommendation with the members' status and enables it only when someone can use it", async () => {
  await act(async () => root.render(<CapabilityCenterWorkspace isOpen groupId="g-demo" />));
  const mcp = [...host.querySelectorAll("button")].find((button) =>
    button.textContent?.includes("capabilityCenter.sections.mcp.label"),
  );
  await act(async () => mcp?.click());

  // Desktop and phone layouts are both in the DOM; CSS shows one of them.
  const rows = (name: string) =>
    [...host.querySelectorAll<HTMLElement>('[role="button"]')].filter((row) =>
      row.textContent?.includes(name),
    );
  const enableButtons = (row: HTMLElement) =>
    row.querySelectorAll('button[aria-label="capabilityCenter.enable.enable"]').length;
  const status = (row: HTMLElement) =>
    row.querySelector("[title^='capabilityCenter.recommended.dot.']")?.getAttribute("title");

  const setupRows = rows("needs-a-setting");
  const usableRows = rows("usable");
  expect(setupRows).toHaveLength(2);
  expect(usableRows).toHaveLength(2);
  for (const row of setupRows) {
    expect(status(row)).toBe("capabilityCenter.recommended.dot.setup");
    expect(enableButtons(row)).toBe(0);
  }
  for (const row of usableRows) {
    expect(status(row)).toBe("capabilityCenter.recommended.dot.available");
    expect(enableButtons(row)).toBe(1);
  }
  expect(host.textContent).not.toContain("capabilityCenter.status.ready");
  expect(host.textContent).not.toContain("cccc_recommended");
});
