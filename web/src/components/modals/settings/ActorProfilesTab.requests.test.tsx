// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ActorProfilesTab } from "./ActorProfilesTab";
import type { ActorProfile } from "../../../types";

const fixture = vi.hoisted(() => ({ keys: vi.fn(), save: vi.fn(), env: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../../services/api", () => ({
  fetchProfilePrivateEnvKeys: fixture.keys,
  saveProfile: fixture.save,
  updateProfilePrivateEnv: fixture.env,
  fetchWebAccessSession: vi.fn(async () => ({ ok: true, result: {} })),
  listProfiles: vi.fn(async () => ({ ok: true, result: { profiles: [] } })),
}));
vi.mock("../../CapabilityPicker", () => ({ CapabilityPicker: () => null }));
vi.mock("../../SelectCombobox", () => ({ SelectCombobox: () => null }));
vi.mock("../RuntimeSelector", () => ({ RuntimeSelector: () => null }));
vi.mock("../AcpRuntimeMode", () => ({ AcpRuntimeMode: () => null }));
function profile(id: string): ActorProfile {
  return {
    id,
    revision: 1,
    name: id,
    runtime: "codex",
    command: "",
    submit: "enter",
  } as ActorProfile;
}
function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const metadata = (key: string) => ({
  ok: true,
  result: { keys: [key], masked_values: { [key]: "synthetic-mask" } },
});
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  fixture.keys.mockReset().mockResolvedValue(metadata("CURRENT_KEY"));
  fixture.save.mockReset().mockResolvedValue({ ok: true, result: { profile: profile("B") } });
  fixture.env.mockReset().mockResolvedValue({ ok: true, result: {} });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const render = (id: string | null, nonce: number) =>
  act(async () =>
    root.render(
      <ActorProfilesTab
        isDark={false}
        isActive
        scope="global"
        editorOnly
        editorRequest={{ nonce, ...(id ? { profile: profile(id) } : {}) }}
      />,
    ),
  );
const dialog = () => document.querySelector('[role="dialog"]')!;
const input = (value: string) =>
  act(async () => {
    const field = dialog().querySelector(
      'textarea[placeholder="actorProfiles.setSecretsPlaceholder"]',
    )!;
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
      field,
      value,
    );
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });

it("drops Profile A metadata arriving after Profile B and preserves B's new private-env draft", async () => {
  const a = deferred();
  fixture.keys.mockImplementation((id: string) =>
    id === "A" ? a.promise : Promise.resolve(metadata("B_KEY")),
  );
  await render("A", 1);
  await render("B", 2);
  await input("B_DRAFT=synthetic");
  await act(async () => a.resolve(metadata("A_KEY")));
  expect(dialog().textContent).toContain("B_KEY");
  expect(dialog().textContent).not.toContain("A_KEY");
  expect(dialog().querySelector("textarea")!.value).toBe("B_DRAFT=synthetic");
  await act(async () =>
    [...dialog().querySelectorAll("button")]
      .find((button) => button.textContent === "common:save")!
      .click(),
  );
  expect(fixture.save.mock.calls[0][0].id).toBe("B");
  expect(fixture.env.mock.calls[0][0]).toBe("B");
});

it("clears the old Profile badges immediately while the replacement metadata is still loading", async () => {
  await render("A", 1);
  expect(dialog().textContent).toContain("CURRENT_KEY");
  const b = deferred();
  fixture.keys.mockReturnValue(b.promise);
  await render("B", 2);
  expect(dialog().textContent).not.toContain("CURRENT_KEY");
  await act(async () => b.resolve(metadata("B_KEY")));
  expect(dialog().textContent).toContain("B_KEY");
});

it("does not copy an earlier Profile's error into a newly opened editor", async () => {
  const a = deferred();
  fixture.keys.mockReturnValueOnce(a.promise);
  await render("A", 1);
  await render(null, 2);
  await input("NEW_DRAFT=synthetic");
  await act(async () =>
    a.resolve({ ok: false, error: { code: "fixture", message: "obsolete Profile error" } }),
  );
  expect(dialog().textContent).not.toContain("obsolete Profile error");
  expect(dialog().textContent).not.toContain("CURRENT_KEY");
  expect(dialog().querySelector("textarea")!.value).toBe("NEW_DRAFT=synthetic");
});

it("ignores metadata from a closed editor when it is subsequently reopened as a new Profile", async () => {
  const a = deferred();
  fixture.keys.mockReturnValueOnce(a.promise);
  await render("A", 1);
  await act(async () =>
    [...dialog().querySelectorAll("button")]
      .find((button) => button.textContent === "common:cancel")!
      .click(),
  );
  await act(async () => a.resolve(metadata("A_KEY")));
  await render(null, 2);
  expect(dialog().textContent).not.toContain("A_KEY");
});
