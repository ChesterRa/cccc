// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import type { MessagingTab } from "./modals/settings/MessagingTab";
import type { DeliveryTab } from "./modals/settings/DeliveryTab";
import type { GroupSettings } from "../types";
import { useModalStore } from "../stores";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";

const tabs = vi.hoisted(() => ({
  messaging: null as ComponentProps<typeof MessagingTab> | null,
  delivery: null as ComponentProps<typeof DeliveryTab> | null,
}));
vi.mock("./modals/settings/MessagingTab", () => ({
  MessagingTab: (props: ComponentProps<typeof MessagingTab>) => {
    tabs.messaging = props;
    return <button onClick={() => void props.onSave()}>Save messaging</button>;
  },
}));
vi.mock("./modals/settings/DeliveryTab", () => ({
  DeliveryTab: (props: ComponentProps<typeof DeliveryTab>) => {
    tabs.delivery = props;
    return <button onClick={() => void props.onSave()}>Save delivery</button>;
  },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../services/api", async (original) => ({
  ...(await original<typeof import("../services/api")>()),
  fetchWebAccessSession: async () => ({ ok: true, result: { can_access_global_settings: true } }),
  fetchIMStatus: async () => ({ ok: true, result: {} }),
  fetchActors: async () => ({ ok: true, result: { actors: [] } }),
  fetchIMConfig: async () => ({ ok: true, result: { im: null } }),
  fetchObservability: async () => ({
    ok: false,
    error: { code: "fixture", message: "Unavailable" },
  }),
}));

const settings: GroupSettings = {
  default_send_to: "foreman",
  actor_idle_timeout_seconds: 0,
  keepalive_delay_seconds: 120,
  keepalive_max_per_actor: 3,
  silence_timeout_seconds: 0,
  help_nudge_interval_seconds: 600,
  help_nudge_min_messages: 10,
  mail_notice_after_seconds: 1800,
  reply_notice_after_seconds: 900,
  terminal_transcript_visibility: "foreman",
  terminal_transcript_notify_tail: false,
  terminal_transcript_notify_lines: 20,
};

describe("settings draft and save continuity", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const save = vi.fn<(patch: Partial<GroupSettings>) => Promise<boolean>>();
  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    save.mockReset().mockResolvedValue(true);
    writeSettingsLastLocation({ scope: "group", groupTab: "messaging", globalTab: "account" });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    window.localStorage.clear();
    useModalStore.setState({ settingsTarget: null });
  });
  const render = async (value = settings, groupId = "g1") => {
    await act(async () =>
      root.render(
        <SettingsModal
          isOpen
          onClose={() => {}}
          settings={value}
          onUpdateSettings={save}
          busy={false}
          isDark={false}
          groupId={groupId}
        />,
      ),
    );
    await vi.waitFor(() => expect(host.textContent).toContain("Save messaging"));
  };
  it("keeps dirty fields through refresh while accepting changes to clean fields", async () => {
    await render();
    await act(async () => tabs.messaging!.setDefaultSendTo("broadcast"));
    await render({ ...settings, mail_notice_after_seconds: 300 });
    expect(tabs.messaging!.defaultSendTo).toBe("broadcast");
    await act(async () =>
      useModalStore.getState().openSettingsTarget({ scope: "group", tab: "delivery" }),
    );
    await vi.waitFor(() => expect(host.textContent).toContain("Save delivery"));
    expect(tabs.delivery!.mailNoticeAfterSeconds).toBe(300);
    await act(async () => tabs.delivery!.onSave());
    expect(save).toHaveBeenCalledWith({
      mail_notice_after_seconds: 300,
      reply_notice_after_seconds: 900,
    });
    await act(async () =>
      useModalStore.getState().openSettingsTarget({ scope: "group", tab: "messaging" }),
    );
    await vi.waitFor(() => expect(host.textContent).toContain("Save messaging"));
    expect(tabs.messaging!.defaultSendTo).toBe("broadcast");
    await render(settings, "g2");
    expect(tabs.messaging!.defaultSendTo).toBe("foreman");
  });
  it("reports confirmed save and failure locally without moving focus or losing the draft", async () => {
    await render();
    await act(async () => tabs.messaging!.setDefaultSendTo("broadcast"));
    const button = host.querySelector<HTMLButtonElement>("button:last-child")!;
    button.focus();
    await act(async () => tabs.messaging!.onSave());
    expect(host.querySelector('[role="status"]')?.textContent).toBe("saveFeedback.saved");
    expect(document.activeElement).toBe(button);
    save.mockResolvedValueOnce(false);
    await act(async () => tabs.messaging!.onSave());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("saveFeedback.failed");
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(tabs.messaging!.defaultSendTo).toBe("broadcast");
  });

  it("retains server errors for retry and ignores a previous Group's late result", async () => {
    await render();
    save.mockRejectedValueOnce(new Error("Permission changed. Sign in again."));
    await act(async () => tabs.messaging!.onSave());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "Permission changed. Sign in again.",
    );
    let complete!: (value: boolean) => void;
    save.mockImplementationOnce(
      () =>
        new Promise<boolean>((resolve) => {
          complete = resolve;
        }),
    );
    let pending!: void | Promise<void>;
    await act(async () => {
      pending = tabs.messaging!.onSave();
    });
    await render(settings, "g2");
    await act(async () => {
      complete(true);
      await pending;
    });
    expect(host.querySelector('[role="status"], [role="alert"]')).toBeNull();
  });
});
