// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SourcesSummary, SourcesView } from "./CapabilitySourcesView";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { count?: number }) =>
      key === "capabilityCenter.sources.records" ? `${args?.count} records` : key,
  }),
}));

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

it("shows source counts and keeps deletion without presenting a source toggle", async () => {
  const sources = {
    cccc_builtin: { source_id: "cccc_builtin", record_count: 2 },
    manual_import: { source_id: "manual_import", record_count: 3 },
  };
  const onDelete = vi.fn();
  await act(async () =>
    root.render(
      <>
        <SourcesView
          sources={sources}
          sourceInstances={[]}
          busyKey=""
          onDelete={onDelete}
          onDeleteInstance={vi.fn()}
        />
        <SourcesSummary sources={sources} />
      </>,
    ),
  );
  expect(host.textContent).toContain("cccc_builtin");
  expect(host.textContent).toContain("2 records");
  expect(host.textContent).toContain("manual_import");
  expect(host.textContent).toContain("3 records");
  expect(host.textContent).not.toContain("capabilityCenter.sources.sync");
  expect(host.textContent).not.toContain("capabilityCenter.sources.instances");
  expect(host.textContent).not.toContain("capabilityCenter.status.enabled");
  expect(host.textContent).not.toContain("capabilityCenter.status.disabled");
  const buttons = host.querySelectorAll<HTMLButtonElement>("button");
  expect(buttons).toHaveLength(1);
  expect(buttons[0].getAttribute("aria-label")).toBe("capabilityCenter.sources.delete");
  await act(async () => buttons[0].click());
  expect(onDelete).toHaveBeenCalledExactlyOnceWith(sources.manual_import);
});
