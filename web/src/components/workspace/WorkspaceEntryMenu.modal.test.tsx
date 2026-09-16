// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { MobilePresentationSurface } from "../presentation/MobilePresentationSurface";
import { WorkspaceEntryMenu } from "./WorkspaceEntryMenu";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
  }),
}));

it("closes only the row menu on Escape inside the mobile file surface", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const [open, setOpen] = useState(true);
    const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);
    return (
      <MobilePresentationSurface
        isOpen={open}
        isDark={false}
        label="Files"
        surface="files"
        onClose={() => setOpen(false)}
      >
        <button type="button" onClick={(event) => setAnchor(event.currentTarget)}>
          Row actions
        </button>
        {anchor && (
          <WorkspaceEntryMenu
            anchor={anchor}
            x={0}
            y={0}
            label="Row actions"
            isDark={false}
            items={[{ key: "copy", label: "Copy path" }]}
            onClose={() => setAnchor(null)}
          />
        )}
      </MobilePresentationSurface>
    );
  }
  try {
    await act(async () => root.render(<Harness />));
    const trigger = document.querySelector<HTMLButtonElement>(
      '[data-mobile-surface="files"] button',
    )!;
    await act(async () => trigger.click());
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    await act(async () =>
      document
        .querySelector('[role="menuitem"]')!
        .dispatchEvent(
          new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
        ),
    );
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.querySelector('[data-mobile-surface="files"]')).not.toBeNull();
    expect(document.activeElement).toBe(trigger);
    await act(async () =>
      trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
    );
    expect(document.querySelector('[data-mobile-surface="files"]')).toBeNull();
    expect(document.body.style.position).not.toBe("fixed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("cancels a dirty reload without closing the surrounding mobile file surface", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const [open, setOpen] = useState(true);
    return (
      <MobilePresentationSurface
        isOpen={open}
        isDark={false}
        label="Files"
        surface="files"
        onClose={() => setOpen(false)}
      >
        <WorkspaceFileViewer
          groupId="fixture"
          file={{
            path: "notes.txt",
            scope_key: "fixture",
            scope_url: "/repo",
            content: "disk",
            sha256: "old",
            bytes: 4,
            mime_type: "text/plain",
            binary: false,
            truncated: false,
          }}
          draft="retained desktop edits"
          setDraft={() => undefined}
          isDark={false}
          readOnly
          saving={false}
          error=""
          conflict={false}
          onClose={() => undefined}
          onSave={async () => true}
          onReload={() => undefined}
          onAttach={() => undefined}
        />
      </MobilePresentationSurface>
    );
  }
  try {
    await act(async () => root.render(<Harness />));
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[title="Reload file"]')!.click(),
    );
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(2);
    const cancel = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "Cancel",
    )!;
    await act(async () =>
      cancel.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
    );
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(document.querySelector('[data-mobile-surface="files"]')).not.toBeNull();
    expect(document.body.textContent).toContain("retained desktop edits");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
