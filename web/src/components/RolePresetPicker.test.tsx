// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { BUILTIN_ROLE_PRESETS } from "../utils/rolePresets";
import { RolePresetPicker } from "./RolePresetPicker";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { defaultValue?: string }) => values?.defaultValue ?? key,
  }),
}));

describe("RolePresetPicker", () => {
  const host = document.createElement("div");
  let root: Root;
  const [first, second] = BUILTIN_ROLE_PRESETS;

  function Fixture({ initial }: { initial: string }) {
    const [draft, setDraft] = useState(initial);
    return (
      <>
        <RolePresetPicker label="Notes" draftValue={draft} onChangeDraft={setDraft} />
        <output>{draft}</output>
      </>
    );
  }
  const draft = () => host.querySelector("output")!.textContent;
  const trigger = () => host.querySelector<HTMLButtonElement>('button[role="combobox"]')!;
  const choose = async (label: string) => {
    trigger().focus();
    await act(async () => trigger().click());
    // Radix mounts its popover in a portal on the next tick.
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
    const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((node) =>
      node.textContent?.includes(label),
    )!;
    await act(async () => option.click());
  };

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it("fills an empty draft as soon as a preset is chosen, with no second step", async () => {
    const confirm = vi.fn();
    vi.stubGlobal("confirm", confirm);
    await act(async () => root.render(<Fixture initial="" />));

    await choose(first.name);

    expect(draft()).toBe(first.content);
    expect(confirm).not.toHaveBeenCalled();
    expect(trigger().textContent).toContain(first.name);
    expect(host.querySelector('[role="status"]')?.textContent).toBe("rolePresetAppliedNotice");
    // The picker is the only control: there is no separate apply button to press.
    expect(host.querySelectorAll("button")).toHaveLength(1);
  });

  it("asks before replacing a written draft and changes nothing when declined", async () => {
    const confirm = vi.fn(() => false);
    vi.stubGlobal("confirm", confirm);
    await act(async () => root.render(<Fixture initial="my own notes" />));

    await choose(first.name);

    expect(confirm).toHaveBeenCalledOnce();
    expect(draft()).toBe("my own notes");
    expect(trigger().textContent).not.toContain(first.name);
    expect(host.querySelector('[role="status"]')).toBeNull();
  });

  it("replaces a written draft once the replacement is confirmed", async () => {
    vi.stubGlobal(
      "confirm",
      vi.fn(() => true),
    );
    await act(async () => root.render(<Fixture initial={first.content} />));

    await choose(second.name);

    expect(draft()).toBe(second.content);
    expect(trigger().textContent).toContain(second.name);
  });
});
