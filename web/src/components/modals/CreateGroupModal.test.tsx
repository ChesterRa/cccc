// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vite-plus/test";
import enModals from "../../i18n/locales/en/modals.json";
import { CreateGroupModal } from "./CreateGroupModal";

const i18n = createInstance();
let host: HTMLDivElement;
let root: Root;

beforeAll(async () => {
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      resources: { en: { modals: enModals } },
      defaultNS: "modals",
      interpolation: { escapeValue: false },
    });
});

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function Harness() {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  return (
    <CreateGroupModal
      isOpen
      isDark={false}
      busy=""
      dirSuggestions={[]}
      dirItems={[]}
      currentDir=""
      parentDir={null}
      showDirBrowser={false}
      createGroupPath={path}
      setCreateGroupPath={setPath}
      createGroupName={name}
      setCreateGroupName={setName}
      creatingDirectory={false}
      onFetchDirContents={vi.fn()}
      onCreateDirectory={async () => false}
      onCreateGroup={vi.fn()}
      onClose={vi.fn()}
      onCancelAndReset={vi.fn()}
    />
  );
}

function inputs(): { path: HTMLInputElement; name: HTMLInputElement } {
  const path = document.querySelector<HTMLInputElement>(
    `input[placeholder="${enModals.createGroup.pathPlaceholder}"]`,
  );
  const name = document.querySelector<HTMLInputElement>(
    `input[placeholder="${enModals.createGroup.groupNamePlaceholder}"]`,
  );
  if (!path || !name) throw new Error("create group inputs not found");
  return { path, name };
}

async function typeInto(input: HTMLInputElement, text: string) {
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  for (const char of text) {
    await act(async () => {
      setValue?.call(input, input.value + char);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
}

async function replaceValue(input: HTMLInputElement, value: string) {
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setValue?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function mount() {
  await act(async () =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <Harness />
      </I18nextProvider>,
    ),
  );
}

it("keeps the derived group name in step with a path typed one key at a time", async () => {
  await mount();
  await typeInto(inputs().path, "/tmp/demo_project");
  expect(inputs().name.value).toBe("demo_project");
  await replaceValue(inputs().path, "/tmp/other");
  expect(inputs().name.value).toBe("other");
});

it("keeps a group name the user typed when the path changes", async () => {
  await mount();
  await typeInto(inputs().path, "/tmp/demo");
  await replaceValue(inputs().name, "My team");
  await typeInto(inputs().path, "_two");
  expect(inputs().path.value).toBe("/tmp/demo_two");
  expect(inputs().name.value).toBe("My team");
});
