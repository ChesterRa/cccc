// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { AcpPermissionList } from "./AcpPermissionList";
import type { AcpPermission } from "../../services/api/codexVoice";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("ACP user decisions", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onRespond = vi.fn();
  const onInteract = vi.fn();
  const request: AcpPermission = {
    request_id: "opaque",
    kind: "question",
    title: "Choose",
    questions: [
      {
        id: "first",
        prompt: "First?",
        options: [
          { id: "a", label: "Alpha" },
          { id: "b", label: "Beta" },
        ],
      },
      {
        id: "second",
        prompt: "Second?",
        allowMultiple: true,
        options: [
          { id: "c", label: "Charlie" },
          { id: "d", label: "Delta" },
        ],
      },
    ],
  };
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.clearAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  const render = (permissions: AcpPermission[], generation = "one", disabled = false) =>
    act(async () =>
      root.render(
        <AcpPermissionList
          key={generation}
          permissions={permissions}
          disabled={disabled}
          onRespond={onRespond}
          onInteract={onInteract}
        />,
      ),
    );
  it("requires explicit choices for every question and preserves multiple selections", async () => {
    await render([request]);
    const submit = host.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(submit.disabled).toBe(true);
    expect(onInteract).not.toHaveBeenCalled();
    const inputs = host.querySelectorAll<HTMLInputElement>("input");
    await act(async () => inputs[1].click());
    expect(submit.disabled).toBe(true);
    await act(async () => {
      inputs[2].click();
      inputs[3].click();
    });
    expect(submit.disabled).toBe(false);
    await act(async () => submit.click());
    expect(onInteract).toHaveBeenCalledWith("opaque", {
      outcome: {
        outcome: "answered",
        answers: [
          { questionId: "first", selectedOptionIds: ["b"] },
          { questionId: "second", selectedOptionIds: ["c", "d"] },
        ],
      },
    });
    expect(onRespond).not.toHaveBeenCalled();
  });
  it("clears choices on generation change and supports a deliberate skip", async () => {
    await render([request]);
    await act(async () => host.querySelector<HTMLInputElement>("input")!.click());
    await render([request], "two");
    expect(host.querySelector<HTMLInputElement>("input")!.checked).toBe(false);
    const skip = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "acpControls.skip",
    )!;
    await act(async () => skip.click());
    expect(onInteract).toHaveBeenCalledWith("opaque", { outcome: { outcome: "skipped" } });
  });
  it("clears an old turn's choices when its replacement has the same question ids", async () => {
    await render([{ ...request, request_id: "old-turn:unique-request" }]);
    await act(async () => host.querySelector<HTMLInputElement>("input")!.click());
    await render([{ ...request, request_id: "new-turn:unique-request" }]);
    expect(host.querySelector<HTMLInputElement>("input")!.checked).toBe(false);
    const skip = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "acpControls.skip",
    )!;
    await act(async () => skip.click());
    expect(onInteract).toHaveBeenCalledExactlyOnceWith("new-turn:unique-request", {
      outcome: { outcome: "skipped" },
    });
  });
  it("keeps plan confirmation separate from tool permission and disables pending writes", async () => {
    const plan = { request_id: "plan", kind: "plan", title: "Inspect", plan: "1. Read evidence" };
    await render([plan]);
    expect(onInteract).not.toHaveBeenCalled();
    expect(host.textContent).toContain("1. Read evidence");
    const reject = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "acpControls.rejectPlan",
    )!;
    await act(async () => reject.click());
    expect(onInteract).toHaveBeenCalledWith("plan", { outcome: { outcome: "rejected" } });
    await render([plan], "one", true);
    expect([...host.querySelectorAll("button")].every((button) => button.disabled)).toBe(true);
    expect(onRespond).not.toHaveBeenCalled();
  });
  it("submits Pi's selected option value and cancels only the current request", async () => {
    await render([
      {
        request_id: "pi-choice",
        kind: "pi_select",
        title: "Choose scope",
        options: ["Current Group", "All Groups"],
      } as AcpPermission,
    ]);
    const submit = host.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(submit?.disabled).toBe(true);
    await act(async () =>
      host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1].click(),
    );
    await act(async () => submit.click());
    expect(onInteract).toHaveBeenCalledWith("pi-choice", { value: "All Groups" });
    await act(async () =>
      [...host.querySelectorAll("button")]
        .find((item) => item.textContent === "acpControls.cancelInteraction")!
        .click(),
    );
    expect(onInteract).toHaveBeenLastCalledWith("pi-choice", { cancelled: true });
    expect(onRespond).not.toHaveBeenCalled();
  });
  it.each(["pi_input", "pi_editor"])(
    "preserves %s's prefill and submits edited text",
    async (kind) => {
      await render([
        {
          request_id: "pi-text",
          kind,
          title: "Additional instruction",
          message: "Provide details",
          placeholder: "Enter reply",
          prefill: "Initial text",
        } as AcpPermission,
      ]);
      const input = host.querySelector<HTMLInputElement | HTMLTextAreaElement>(
        'input[type="text"], textarea',
      )!;
      expect(input?.value).toBe("Initial text");
      expect(input.placeholder).toBe("Enter reply");
      expect(host.textContent).toContain("Provide details");
      await act(async () => {
        Object.getOwnPropertyDescriptor(
          kind === "pi_editor" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype,
          "value",
        )!.set!.call(input, "Updated text");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () =>
        host.querySelector<HTMLButtonElement>('button[type="submit"]')!.click(),
      );
      expect(onInteract).toHaveBeenCalledWith("pi-text", { value: "Updated text" });
      await render([
        {
          request_id: "pi-new-text",
          kind,
          title: "New task",
          prefill: "New prefill",
        } as AcpPermission,
      ]);
      expect(
        host.querySelector<HTMLInputElement | HTMLTextAreaElement>('input[type="text"], textarea')!
          .value,
      ).toBe("New prefill");
    },
  );
  it("uses explicit confirm/decline for Pi and disables pending interactions", async () => {
    const confirmation = {
      request_id: "pi-confirm",
      kind: "pi_confirm",
      title: "Proceed?",
      message: "This runs the selected task",
    } as AcpPermission;
    await render([confirmation]);
    await act(async () =>
      [...host.querySelectorAll("button")]
        .find((item) => item.textContent === "acpControls.confirm")!
        .click(),
    );
    expect(onRespond).toHaveBeenCalledWith("pi-confirm", true);
    expect(onInteract).not.toHaveBeenCalled();
    await render([confirmation], "one", true);
    expect([...host.querySelectorAll("button")].every((item) => item.disabled)).toBe(true);
  });
});
