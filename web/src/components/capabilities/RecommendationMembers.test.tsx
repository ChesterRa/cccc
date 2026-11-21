// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import type { CapabilityMemberProjection, CapabilityOverviewItem } from "../../types";
import { RecommendationMembers } from "./CapabilityCenterWorkspace";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const KEY = "capabilityCenter.recommended.member";

function member(
  actorId: string,
  fields: Partial<CapabilityMemberProjection>,
): CapabilityMemberProjection {
  return {
    actor_id: actorId,
    availability: "compatible",
    blocked: false,
    enabled: false,
    activation_pending: false,
    reason_codes: [],
    enabled_scope: null,
    enabled_scopes: [],
    last_call: null,
    ...fields,
  } as CapabilityMemberProjection;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("lists every scope a member enabled, whether or not the member can run it now", async () => {
  const row = {
    capability_id: "skill:recommended:demo",
    group_enabled: true,
    members: [
      member("unsure", {
        availability: "unknown",
        enabled: true,
        enabled_scope: "session",
        enabled_scopes: ["group", "actor", "session"],
      }),
      member("stopped", {
        blocked: true,
        enabled: true,
        enabled_scope: "group",
        enabled_scopes: ["group"],
      }),
      member("ready", { enabled: true, enabled_scope: "group", enabled_scopes: ["group"] }),
    ],
  } as CapabilityOverviewItem;

  await act(async () => root.render(<RecommendationMembers row={row} />));

  const rows = [...host.querySelectorAll("li")].map((item) => item.textContent || "");
  expect(rows[0]).toBe(`unsure ${KEY}.unknown · ${KEY}.session · ${KEY}.actor · ${KEY}.group`);
  expect(rows[1]).toBe(`stopped ${KEY}.blocked · ${KEY}.group`);
  expect(rows[2]).toBe(`ready ${KEY}.compatible · ${KEY}.group`);
});
