// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { emptyActorSecretChanges, type ActorSecretChanges } from "./actorSecretManagerModel";
import { DeepSeekConnectionFields } from "./DeepSeekConnectionFields";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { value?: string }) =>
      options?.value ? `${key}:${options.value}` : key,
  }),
}));

describe("DeepSeek connection fields", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  function render(
    props: Partial<Parameters<typeof DeepSeekConnectionFields>[0]> & {
      onChange: (changes: ActorSecretChanges) => void;
    },
  ) {
    act(() =>
      root.render(<DeepSeekConnectionFields changes={emptyActorSecretChanges()} {...props} />),
    );
  }

  const buttons = (label: string) =>
    [...host.querySelectorAll("button")].filter((button) => button.textContent === label);

  it("removes a saved address or model on Use default instead of saving today's default", () => {
    const onChange = vi.fn();
    render({ keys: ["DEEPSEEK_BASE_URL", "DEEPSEEK_MODEL"], onChange });
    const [hostDefault, modelDefault] = buttons("deepseek.useDefault");
    act(() => modelDefault.click());
    expect(onChange).toHaveBeenLastCalledWith({
      setVars: {},
      unsetKeys: ["DEEPSEEK_MODEL"],
      clearAll: false,
    });
    act(() => hostDefault.click());
    expect(onChange).toHaveBeenLastCalledWith({
      setVars: {},
      unsetKeys: ["DEEPSEEK_BASE_URL"],
      clearAll: false,
    });
  });

  it("drops a value typed in this form on Use default and has nothing to reset otherwise", () => {
    const onChange = vi.fn();
    render({ onChange });
    expect(buttons("deepseek.useDefault").map((button) => button.disabled)).toEqual([true, true]);
    render({
      changes: { ...emptyActorSecretChanges(), setVars: { DEEPSEEK_MODEL: "channel/custom" } },
      onChange,
    });
    const [, modelDefault] = buttons("deepseek.useDefault");
    expect(modelDefault.disabled).toBe(false);
    act(() => modelDefault.click());
    expect(onChange).toHaveBeenLastCalledWith(emptyActorSecretChanges());
  });

  it("never shows a saved key and stages its removal on request", () => {
    const onChange = vi.fn();
    render({ keys: ["DEEPSEEK_API_KEY"], onChange });
    const key = host.querySelector<HTMLInputElement>('input[aria-label="deepseek.apiKey"]');
    expect(key?.value).toBe("");
    expect(key?.placeholder).toBe("deepseek.keepSaved");
    act(() => buttons("deepseek.removeKey")[0].click());
    expect(onChange).toHaveBeenLastCalledWith({
      setVars: {},
      unsetKeys: ["DEEPSEEK_API_KEY"],
      clearAll: false,
    });
  });

  it("leaves saving to the dialog and says when a new Agent's values are saved", () => {
    const pending = {
      ...emptyActorSecretChanges(),
      setVars: { DEEPSEEK_MODEL: "deepseek-v4-pro" },
    };
    render({ keys: ["DEEPSEEK_MODEL"], changes: pending, onChange: vi.fn() });
    expect(buttons("common:save")).toHaveLength(0);
    expect(buttons("saveAndRestart")).toHaveLength(0);
    expect(host.textContent).not.toContain("deepseek.createPendingHint");
    render({ changes: pending, creating: true, onChange: vi.fn() });
    expect(host.querySelector('[role="status"]')?.textContent).toBe("deepseek.createPendingHint");
  });

  it("does not stage a value made only of spaces", () => {
    const onChange = vi.fn();
    render({ keys: ["DEEPSEEK_API_KEY"], onChange });
    const key = host.querySelector<HTMLInputElement>('input[aria-label="deepseek.apiKey"]')!;
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      setValue.call(key, "   ");
      key.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(onChange).toHaveBeenLastCalledWith(emptyActorSecretChanges());
  });

  it("suggests the documented Flash and Pro model names", () => {
    render({ onChange: vi.fn() });
    const options = [...host.querySelectorAll("datalist option")].map(
      (option) => (option as HTMLOptionElement).value,
    );
    expect(options).toEqual(["deepseek-flash", "deepseek-v4-pro"]);
    const model = host.querySelectorAll<HTMLInputElement>("input")[1];
    expect(model.placeholder).toBe("deepseek.example:deepseek-flash");
  });
});
