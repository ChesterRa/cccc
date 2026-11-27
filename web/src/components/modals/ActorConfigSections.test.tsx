// @vitest-environment happy-dom
import { act, type ReactNode, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";

import { ActorConfigAdvanced, ActorConfigForm, ActorConfigViews } from "./ActorConfigSections";

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
        summary={changed ? "2 environment variables" : "Command · Capabilities"}
        changed={changed}
        open={open}
        onOpenChange={setOpen}
      >
        <input aria-label="Command" defaultValue="" />
      </ActorConfigAdvanced>
    );
  }
  const toggle = () => host.querySelector<HTMLButtonElement>("button[aria-expanded]")!;
  const panel = () => document.getElementById(toggle().getAttribute("aria-controls")!)!;

  it("starts closed, says what is inside, and opens on request", async () => {
    await render(<Fixture />);

    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    expect(panel().hidden).toBe(true);
    expect(toggle().textContent).toBe("AdvancedCommand · Capabilities");

    await act(async () => toggle().click());

    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(panel().hidden).toBe(false);
    // Once the content is visible the list of what is inside would only repeat it.
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

describe("ActorConfigViews", () => {
  const view = (name: string) =>
    host.querySelector<HTMLElement>(`[data-actor-config-view="${name}"]`)!;
  const reachable = (name: string) => !view(name).hasAttribute("inert");

  it("shows one view at a time and keeps the other out of reach of keyboard and pointer", async () => {
    await render(
      <ActorConfigViews showAdvanced={false} form={<input />} advanced={<textarea />} />,
    );
    expect([reachable("form"), reachable("advanced")]).toEqual([true, false]);

    await render(<ActorConfigViews showAdvanced form={<input />} advanced={<textarea />} />);
    expect([reachable("form"), reachable("advanced")]).toEqual([false, true]);
  });

  it("keeps drafts in both views across a switch", async () => {
    await render(
      <ActorConfigViews showAdvanced={false} form={<input />} advanced={<textarea />} />,
    );
    const name = host.querySelector("input")!;
    const environment = host.querySelector("textarea")!;
    name.value = "codex-3";
    environment.value = "FIXTURE=1";

    await render(<ActorConfigViews showAdvanced form={<input />} advanced={<textarea />} />);
    await render(
      <ActorConfigViews showAdvanced={false} form={<input />} advanced={<textarea />} />,
    );

    expect(host.querySelector("input")).toBe(name);
    expect(name.value).toBe("codex-3");
    expect(host.querySelector("textarea")).toBe(environment);
    expect(environment.value).toBe("FIXTURE=1");
  });

  it("never hides the form when there are no advanced settings to show instead", async () => {
    await render(<ActorConfigViews showAdvanced form={<input />} advanced={null} />);

    expect(reachable("form")).toBe(true);
    expect(view("advanced")).toBeNull();
  });
});
