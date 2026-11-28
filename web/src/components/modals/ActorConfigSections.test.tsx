// @vitest-environment happy-dom
import { act, type ReactNode, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";

import { ActorConfigAdvanced, ActorConfigForm } from "./ActorConfigSections";

const host = document.createElement("div");
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const render = async (node: ReactNode) => act(async () => root.render(node));

describe("ActorConfigForm", () => {
  const rowIds = () =>
    [...host.querySelectorAll<HTMLElement>("[data-actor-config-row]")].map(
      (row) => row.dataset.actorConfigRow,
    );

  it("puts identity beside runtime and the remaining rows below, in reading order", async () => {
    await render(
      <ActorConfigForm
        identityLabel="Basics"
        runtimeLabel="Runtime"
        identity={<span>name</span>}
        runtime={<span>runtime</span>}
        advanced={<span>advanced</span>}
        notes={<span>notes</span>}
        directory={<span>directory</span>}
      />,
    );

    const sections = [...host.querySelectorAll("section")];
    expect(sections.map((section) => section.getAttribute("aria-label"))).toEqual([
      "Basics",
      "Runtime",
    ]);
    expect(sections[0].parentElement).toBe(sections[1].parentElement);
    expect(rowIds()).toEqual(["directory", "notes", "advanced"]);
    expect(host.textContent).toBe("nameruntimedirectorynotesadvanced");
  });

  it("stacks directory and notes with identity, and keeps advanced across both columns", async () => {
    await render(
      <ActorConfigForm
        identityLabel="Basics"
        runtimeLabel="Runtime"
        identity="name"
        runtime="runtime"
        directory="directory"
        notes="notes"
        advanced="advanced"
      />,
    );
    const grid = host.querySelector("[data-actor-config-grid]")!;
    const row = (id: string) => host.querySelector(`[data-actor-config-row="${id}"]`)!;
    // Directory and notes share the two-column grid with identity and runtime, so they sit
    // under identity instead of leaving that column empty beside a long runtime column.
    expect(row("directory").parentElement).toBe(grid);
    expect(row("notes").parentElement).toBe(grid);
    expect(grid.querySelectorAll("section")).toHaveLength(2);
    // Advanced is a row of its own under the grid.
    expect(grid.contains(row("advanced"))).toBe(false);
    expect(grid.nextElementSibling).toBe(row("advanced"));
  });

  it("leaves no empty row behind for a part the Agent does not have", async () => {
    await render(
      <ActorConfigForm
        identityLabel="Basics"
        runtimeLabel="Runtime"
        identity="name"
        runtime="runtime"
        directory={null}
        notes={<span>notes</span>}
        advanced={false}
      />,
    );

    expect(rowIds()).toEqual(["notes"]);
  });
});

describe("ActorConfigAdvanced", () => {
  function Fixture({ changed = false }: { changed?: boolean }) {
    const [open, setOpen] = useState(false);
    return (
      <ActorConfigAdvanced
        title="Advanced"
        summary={changed ? "2 environment variables" : undefined}
        open={open}
        onOpenChange={setOpen}
      >
        <input aria-label="Command" defaultValue="" />
      </ActorConfigAdvanced>
    );
  }
  const toggle = () => host.querySelector<HTMLButtonElement>("button[aria-expanded]")!;
  const panel = () => document.getElementById(toggle().getAttribute("aria-controls")!)!;

  it("starts closed and opens without a summary when there are no staged changes", async () => {
    await render(<Fixture />);

    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    expect(panel().hidden).toBe(true);
    expect(toggle().textContent).toBe("Advanced");

    await act(async () => toggle().click());

    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(panel().hidden).toBe(false);
    expect(toggle().textContent).toBe("Advanced");
  });

  it("keeps a draft typed inside it while it is closed", async () => {
    await render(<Fixture />);
    await act(async () => toggle().click());
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Command"]')!;
    input.value = "codex --fixture";

    await act(async () => toggle().click());
    expect(panel().hidden).toBe(true);
    await act(async () => toggle().click());

    expect(host.querySelector<HTMLInputElement>('input[aria-label="Command"]')).toBe(input);
    expect(input.value).toBe("codex --fixture");
  });

  it("keeps reporting changed settings whether it is open or closed", async () => {
    await render(<Fixture changed />);
    expect(toggle().textContent).toBe("Advanced2 environment variables");
    expect(host.querySelector("[data-changed]")).not.toBeNull();

    await act(async () => toggle().click());

    expect(toggle().textContent).toBe("Advanced2 environment variables");
  });
});
