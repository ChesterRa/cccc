// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { ActorProfile } from "../../../types";

const mocks = vi.hoisted(() => ({
  fetchWebAccessSession: vi.fn(),
  listProfiles: vi.fn(),
  saveProfile: vi.fn(),
  deleteProfile: vi.fn(),
  getProfile: vi.fn(),
  updateProfilePrivateEnv: vi.fn(),
  fetchProfilePrivateEnvKeys: vi.fn(),
  t: (key: string) => key,
}));
const groupStore = vi.hoisted(() => ({
  groups: [] as { group_id: string }[],
  refreshGroups: vi.fn(),
  refreshActors: vi.fn(),
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));

vi.mock("../../../services/api", () => ({
  fetchWebAccessSession: (...args: unknown[]) => mocks.fetchWebAccessSession(...args),
  listProfiles: (...args: unknown[]) => mocks.listProfiles(...args),
  saveProfile: (...args: unknown[]) => mocks.saveProfile(...args),
  deleteProfile: (...args: unknown[]) => mocks.deleteProfile(...args),
  getProfile: (...args: unknown[]) => mocks.getProfile(...args),
  updateProfilePrivateEnv: (...args: unknown[]) => mocks.updateProfilePrivateEnv(...args),
  fetchProfilePrivateEnvKeys: (...args: unknown[]) => mocks.fetchProfilePrivateEnvKeys(...args),
}));

vi.mock("../../../stores", () => ({
  useGroupStore: Object.assign(
    (selector: (state: typeof groupStore) => unknown) => selector(groupStore),
    { getState: () => groupStore },
  ),
}));

vi.mock("../../CapabilityPicker", () => ({ CapabilityPicker: () => null }));
vi.mock("../../SelectCombobox", () => ({ SelectCombobox: () => null }));
vi.mock("../RuntimeSelector", () => ({ RuntimeSelector: () => null }));
vi.mock("../AcpRuntimeMode", () => ({ AcpRuntimeMode: () => null }));

import { ActorProfilesTab } from "./ActorProfilesTab";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeProfile(id: string, name: string): ActorProfile {
  return { id, name, runtime: "codex", revision: 1, scope: "global" } as ActorProfile;
}

describe("ActorProfilesTab async ownership fencing", () => {
  let host: HTMLDivElement;
  let root: Root;

  const renderTab = async (scope: "global" | "my" = "global", isActive = true) => {
    await act(async () => {
      root.render(<ActorProfilesTab isDark={false} isActive={isActive} scope={scope} />);
    });
  };

  beforeEach(() => {
    vi.resetAllMocks();
    groupStore.groups = [];
    mocks.fetchWebAccessSession.mockResolvedValue({
      ok: true,
      result: { web_access_session: { user_id: "u1", current_browser_signed_in: true } },
    });
    mocks.fetchProfilePrivateEnvKeys.mockResolvedValue({
      ok: true,
      result: { keys: [], masked_values: {} },
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it("clears global profiles immediately on scope switch and ignores late global responses", async () => {
    const globalList = deferred<{ ok: true; result: { profiles: ActorProfile[] } }>();
    const myList = deferred<{ ok: true; result: { profiles: ActorProfile[] } }>();
    mocks.listProfiles.mockImplementation((view: string) =>
      view === "global" ? globalList.promise : myList.promise,
    );

    await renderTab("global");
    await renderTab("my");

    await act(async () => {
      globalList.resolve({ ok: true, result: { profiles: [makeProfile("p-global", "Global P")] } });
      await globalList.promise;
    });
    expect(host.textContent).not.toContain("Global P");

    await act(async () => {
      myList.resolve({ ok: true, result: { profiles: [makeProfile("p-my", "My Personal P")] } });
      await myList.promise;
    });
    expect(host.textContent).toContain("My Personal P");
    expect(host.textContent).not.toContain("Global P");
  });

  it("does not close a newly opened editor when an earlier save's follow-up refresh finishes", async () => {
    const followUpList = deferred<{ ok: true; result: { profiles: ActorProfile[] } }>();
    mocks.listProfiles
      .mockResolvedValueOnce({
        ok: true,
        result: { profiles: [makeProfile("p-1", "Profile One")] },
      })
      .mockReturnValueOnce(followUpList.promise)
      .mockResolvedValue({ ok: true, result: { profiles: [makeProfile("p-1", "Profile One")] } });
    mocks.saveProfile.mockResolvedValue({
      ok: true,
      result: { profile: makeProfile("p-1", "Profile One") },
    });

    await renderTab("global");
    const editBtn = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "common:edit",
    )!;
    await act(async () => editBtn.click());

    const saveBtn = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "common:save",
    )!;
    await act(async () => saveBtn.click());
    expect(mocks.saveProfile).toHaveBeenCalledTimes(1);

    // Switch scope to "my" and back to "global", then open a new profile editor
    await renderTab("my");
    await renderTab("global");
    const newBtn = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "actorProfiles.newProfile",
    )!;
    await act(async () => newBtn.click());
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull();

    // Now the old save's follow-up listProfiles resolves
    await act(async () => {
      followUpList.resolve({ ok: true, result: { profiles: [makeProfile("p-1", "Profile One")] } });
      await followUpList.promise;
    });
    // The newly opened editor must remain open!
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it("finishes a usage read independently of an ordinary list refresh", async () => {
    const profile = makeProfile("p1", "Profile One");
    const usage = deferred<{ ok: true; result: { profile: ActorProfile; usage: [] } }>();
    mocks.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [profile] } });
    mocks.getProfile.mockReturnValue(usage.promise);
    const alert = vi.fn();
    vi.stubGlobal("alert", alert);
    await renderTab();
    const button = (label: string) =>
      [...host.querySelectorAll<HTMLButtonElement>("button")].find((e) => e.textContent === label)!;
    const usageButton = button("actorProfiles.viewUsage");
    await act(async () => usageButton.click());
    await act(async () => button("actorProfiles.refresh").click());
    await act(async () => {
      usage.resolve({ ok: true, result: { profile, usage: [] } });
      await usage.promise;
    });
    expect(usageButton.isConnected).toBe(true);
    expect(usageButton.disabled).toBe(false);
    expect(alert).toHaveBeenCalledWith("actorProfiles.usageEmpty");
  });

  it("reconciles independent deletions that finish in reverse order", async () => {
    const profiles = [makeProfile("a", "Profile A"), makeProfile("b", "Profile B")];
    const first = deferred<void>();
    const second = deferred<void>();
    mocks.listProfiles.mockImplementation(async () => ({
      ok: true,
      result: { profiles: [...profiles] },
    }));
    mocks.deleteProfile.mockImplementation(async (id: string) => {
      await (id === "a" ? first.promise : second.promise);
      profiles.splice(
        profiles.findIndex((profile) => profile.id === id),
        1,
      );
      return { ok: true, result: {} };
    });
    vi.stubGlobal("confirm", () => true);
    await renderTab();
    const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")].filter(
      (button) => button.textContent === "common:delete",
    );
    await act(async () => buttons[0].click());
    await act(async () => buttons[1].click());
    await act(async () => second.resolve());
    expect(host.textContent).toContain("Profile A");
    expect(host.textContent).not.toContain("Profile B");
    await act(async () => first.resolve());
    expect(host.textContent).not.toContain("Profile A");
    expect(host.textContent).not.toContain("Profile B");
  });

  it("does not refill another scope when a deletion finishes after switching pages", async () => {
    const deleted = deferred<{ ok: true; result: object }>();
    mocks.listProfiles.mockImplementation(async (scope: string) => ({
      ok: true,
      result: { profiles: [makeProfile(scope, scope === "global" ? "Global P" : "Personal P")] },
    }));
    mocks.deleteProfile.mockReturnValue(deleted.promise);
    vi.stubGlobal("confirm", () => true);
    await renderTab();
    await act(async () =>
      [...host.querySelectorAll<HTMLButtonElement>("button")]
        .find((button) => button.textContent === "common:delete")!
        .click(),
    );
    await renderTab("my");
    const reads = mocks.listProfiles.mock.calls.length;
    await act(async () => deleted.resolve({ ok: true, result: {} }));
    expect(mocks.listProfiles).toHaveBeenCalledTimes(reads);
    expect(host.textContent).toContain("Personal P");
    expect(host.textContent).not.toContain("Global P");
  });

  it("refreshes shared Actor state after accepted force deletion outlives its page", async () => {
    const deleted = deferred<{ ok: true; result: object }>();
    groupStore.groups = [{ group_id: "g1" }];
    mocks.listProfiles.mockResolvedValue({
      ok: true,
      result: { profiles: [makeProfile("p1", "Profile One")] },
    });
    mocks.deleteProfile
      .mockResolvedValueOnce({
        ok: false,
        error: { code: "profile_in_use", details: { usage: [] } },
      })
      .mockReturnValueOnce(deleted.promise);
    vi.stubGlobal("confirm", () => true);
    await renderTab();
    await act(async () =>
      [...host.querySelectorAll<HTMLButtonElement>("button")]
        .find((button) => button.textContent === "common:delete")!
        .click(),
    );
    expect(mocks.deleteProfile).toHaveBeenLastCalledWith(
      "p1",
      expect.objectContaining({ forceDetach: true }),
    );
    await renderTab("global", false);
    await act(async () => deleted.resolve({ ok: true, result: {} }));
    expect(groupStore.refreshGroups).toHaveBeenCalledTimes(1);
    expect(groupStore.refreshActors).toHaveBeenCalledWith("g1");
  });

  it.each([true, false])(
    "finishes a submitted credential write after editor replacement without changing the new editor (ok=%s)",
    async (success) => {
      const saved = makeProfile("p1", "Profile One");
      const pending = deferred<{ ok: true; result: { profile: ActorProfile } }>();
      mocks.saveProfile.mockReturnValue(pending.promise);
      mocks.updateProfilePrivateEnv.mockResolvedValue(
        success
          ? { ok: true, result: { keys: ["SYNTHETIC_SETTING"] } }
          : { ok: false, error: { code: "fixture_failure", message: "Synthetic write failed" } },
      );
      const onSaved = vi.fn();
      const renderEditor = (profile: ActorProfile, nonce: number) =>
        act(async () =>
          root.render(
            <ActorProfilesTab
              isDark={false}
              isActive
              scope="global"
              editorOnly
              editorRequest={{ profile, nonce }}
              onSaved={onSaved}
            />,
          ),
        );
      await renderEditor(saved, 1);
      const text = document.querySelector<HTMLTextAreaElement>(
        'textarea[placeholder="actorProfiles.setSecretsPlaceholder"]',
      )!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
          text,
          "SYNTHETIC_SETTING=fixture",
        );
        text.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () =>
        [...document.querySelectorAll<HTMLButtonElement>("button")]
          .find((e) => e.textContent === "common:save")!
          .click(),
      );
      expect(mocks.saveProfile).toHaveBeenCalledTimes(1);
      await renderEditor(makeProfile("p2", "Profile Two"), 2);
      await act(async () => {
        pending.resolve({ ok: true, result: { profile: { ...saved, revision: 2 } } });
        await pending.promise;
      });
      expect(mocks.updateProfilePrivateEnv).toHaveBeenCalledWith(
        "p1",
        { SYNTHETIC_SETTING: "fixture" },
        [],
        false,
        { scope: "global", ownerId: "" },
      );
      const dialog = document.querySelector('[role="dialog"]')!;
      expect(
        [...dialog.querySelectorAll<HTMLInputElement>("input")].some(
          (e) => e.value === "Profile Two",
        ),
      ).toBe(true);
      expect(dialog.textContent).not.toContain("Synthetic write failed");
      expect(onSaved).not.toHaveBeenCalled();
    },
  );
});
