// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { WebModelConnectorCall } from "../../../services/api";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import { WebModelConnectorDiagnostics } from "./WebModelConnectorDiagnostics";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const render = async (call: Partial<WebModelConnectorCall>) => {
  await act(async () =>
    root.render(
      <WebModelConnectorDiagnostics
        connector={{
          connector_id: "fixture",
          events_log: [
            {
              at: "2026-10-23T13:46:52Z",
              method: "tools/call",
              tool_name: "cccc_runtime_wait_next_turn",
              ok: false,
              reason: null,
              ...call,
            },
          ],
        }}
        disabled={false}
        onRefresh={vi.fn()}
      />,
    ),
  );
};

it.each([
  ["paired_actor_stopped", "actorStopped"],
  ["paired_actor_identity_changed", "bindingChanged"],
  ["paired_actor_runtime_changed", "bindingChanged"],
  ["conversation_not_paired", "conversationUnpaired"],
])("gives a specific recovery hint for %s", async (reason, hint) => {
  await render({ reason });
  expect(host.textContent).toContain(`webModelDiagnostics.reasons.${hint}`);
});

it("keeps identity comparisons collapsed and marks absent optional host fields", async () => {
  await render({
    host_identity: { session: "a".repeat(64), subject: "b".repeat(64), organization: null },
  });
  const details = host.querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain("a".repeat(64));
  expect(details.textContent).toContain("b".repeat(64));
  expect(details.textContent).toContain("webModelDiagnostics.identity.absent");
});

it("does not invent identity information for earlier or subscription records", async () => {
  await render({ method: "events/subscribe", tool_name: undefined });
  expect(host.querySelector("details")).toBeNull();
});
