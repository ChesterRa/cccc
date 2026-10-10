// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../../types";
import { DeliveryTab } from "./DeliveryTab";
import { MessagingTab } from "./MessagingTab";
import { SettingsNavigation } from "./SettingsNavigation";
import { SettingsSaveBar, SettingsSwitch } from "./SettingsKit";
import { TranscriptTab } from "./TranscriptTab";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  Trans: ({ i18nKey }: { i18nKey: string }) => i18nKey,
}));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const render = (node: React.ReactNode) => act(async () => root.render(node));
const click = (element: Element) => act(async () => (element as HTMLElement).click());

describe("settings kit", () => {
  it("toggles a switch only while it is enabled", async () => {
    const onChange = vi.fn();
    await render(<SettingsSwitch checked={false} onChange={onChange} labelledBy="x" />);
    const toggle = host.querySelector('[role="switch"]')!;
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await click(toggle);
    expect(onChange).toHaveBeenCalledWith(true);
    await render(<SettingsSwitch checked onChange={onChange} labelledBy="x" disabled />);
    await click(host.querySelector('[role="switch"]')!);
    expect(onChange).toHaveBeenCalledOnce();
  });

  it("saves from the bar and blocks a second save while one runs", async () => {
    const onSave = vi.fn();
    await render(<SettingsSaveBar label="save" busy={false} onSave={onSave} />);
    await click(host.querySelector("[data-settings-save-bar] button")!);
    expect(onSave).toHaveBeenCalledOnce();
    await render(<SettingsSaveBar label="save" busy onSave={onSave} />);
    const busy = host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")!;
    expect(busy.disabled).toBe(true);
    expect(busy.textContent).toBe("common:saving");
  });

  it("can block saving without claiming a save is running", async () => {
    const onSave = vi.fn();
    await render(<SettingsSaveBar label="save" busy={false} disabled onSave={onSave} />);
    const button = host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")!;
    expect(button.disabled).toBe(true);
    expect(button.textContent).toBe("save");
    await click(button);
    expect(onSave).not.toHaveBeenCalled();
  });
});

describe("settings pages on the kit", () => {
  it("delivery: each labelled field edits its own value and the bar saves", async () => {
    const onSave = vi.fn();
    function Page() {
      const [mail, setMail] = useState(1800);
      const [idle, setIdle] = useState(60);
      const [reply, setReply] = useState(900);
      return (
        <DeliveryTab
          isDark={false}
          busy={false}
          mailNoticeAfterSeconds={mail}
          setMailNoticeAfterSeconds={setMail}
          mailNoticeIdleAfterSeconds={idle}
          setMailNoticeIdleAfterSeconds={setIdle}
          replyNoticeAfterSeconds={reply}
          setReplyNoticeAfterSeconds={setReply}
          onSave={onSave}
        />
      );
    }
    await render(<Page />);
    const labels = [...host.querySelectorAll<HTMLLabelElement>("[data-settings-row] label")];
    expect(labels.map((label) => label.textContent)).toEqual([
      "delivery.mailNotice",
      "delivery.mailIdleNotice",
      "delivery.replyNotice",
    ]);
    const idleInput = host.querySelector<HTMLInputElement>(`#${CSS.escape(labels[1].htmlFor)}`)!;
    expect(idleInput.value).toBe("60");
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      setter.call(idleInput, "45");
      idleInput.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(idleInput.value).toBe("45");
    await click(host.querySelector("[data-settings-save-bar] button")!);
    expect(onSave).toHaveBeenCalledOnce();
  });

  it("messaging: the default recipient is a one-tap choice", async () => {
    const setDefault = vi.fn();
    await render(
      <MessagingTab
        isDark={false}
        busy={false}
        defaultSendTo="foreman"
        setDefaultSendTo={setDefault}
        onSave={vi.fn()}
      />,
    );
    const radios = [...host.querySelectorAll('[role="radio"]')];
    expect(radios.map((radio) => radio.getAttribute("aria-checked"))).toEqual(["true", "false"]);
    await click(radios[1]);
    expect(setDefault).toHaveBeenCalledWith("broadcast");
  });

  it("transcript: notification lines follow the include-tail switch", async () => {
    const setTail = vi.fn();
    const props = {
      isDark: false,
      busy: false,
      groupId: "g1",
      devActors: [] as Actor[],
      terminalVisibility: "foreman" as const,
      setTerminalVisibility: vi.fn(),
      setTerminalNotifyTail: setTail,
      terminalNotifyLines: 20,
      setTerminalNotifyLines: vi.fn(),
      onSaveTranscriptSettings: vi.fn(),
      tailActorId: "",
      setTailActorId: vi.fn(),
      tailMaxChars: 8000,
      setTailMaxChars: vi.fn(),
      tailStripAnsi: true,
      setTailStripAnsi: vi.fn(),
      tailCompact: true,
      setTailCompact: vi.fn(),
      tailText: "",
      tailHint: "",
      tailErr: "",
      tailBusy: false,
      tailCopyInfo: "",
      onLoadTail: vi.fn(),
      onCopyTail: vi.fn(),
      onClearTail: vi.fn(),
    };
    await render(<TranscriptTab {...props} terminalNotifyTail={false} />);
    // Every visible label must point at a real control; the actor picker is named by aria-label.
    for (const label of host.querySelectorAll("label[for]")) {
      expect(document.getElementById(label.getAttribute("for")!)).not.toBeNull();
    }
    expect(host.querySelector('[aria-label="transcript.actor"]')).not.toBeNull();
    const lines = () => host.querySelector<HTMLInputElement>('input[type="number"][max="80"]')!;
    expect(lines().disabled).toBe(true);
    await click(host.querySelector('[role="switch"]')!);
    expect(setTail).toHaveBeenCalledWith(true);
    await render(<TranscriptTab {...props} terminalNotifyTail />);
    expect(lines().disabled).toBe(false);
    // Clearing sits in its own danger card and needs a chosen actor.
    const danger = host.querySelector('[data-settings-card="danger"] button') as HTMLButtonElement;
    expect(danger.disabled).toBe(true);
  });

  it("navigation: the scope switch is a pressed-button group that names what it applies to", async () => {
    const onScope = vi.fn();
    const renderNav = (globalEnabled: boolean) =>
      render(
        <SettingsNavigation
          isDark={false}
          groupId="g1"
          groupTitle="cccc"
          scope="group"
          scopeRootUrl=""
          globalEnabled={globalEnabled}
          tabs={[{ id: "delivery", label: "delivery" }]}
          activeTab="delivery"
          onScopeChange={onScope}
          onTabChange={vi.fn()}
        />,
      );
    await renderNav(false);
    const scopeSwitch = host.querySelector("[data-settings-scope-switch]") as HTMLElement;
    // A tablist would promise arrow-key navigation and tabpanels; this is a toggle group.
    expect(scopeSwitch.getAttribute("role")).toBe("group");
    expect(host.querySelector("[role=tablist], [role=tab]")).toBeNull();
    const buttons = () => [...scopeSwitch.querySelectorAll<HTMLButtonElement>("button")];
    expect(buttons().map((b) => b.getAttribute("aria-pressed"))).toEqual(["true", "false"]);
    expect(buttons()[1].disabled).toBe(true);
    expect(host.textContent).toContain("navigation.appliesToGroup");
    await click(buttons()[1]);
    expect(onScope).not.toHaveBeenCalled();

    await renderNav(true);
    await click(buttons()[1]);
    expect(onScope).toHaveBeenCalledWith("global");
  });
});
