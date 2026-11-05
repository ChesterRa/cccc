// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { RuntimeCategoryPicker } from "./RuntimeCategoryPicker";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("prevents direct native and Web Model switches while retaining managed-to-Web selection", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const change = vi.fn();
  const find = (text: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (item) => item.textContent === text,
    )!;
  try {
    await act(async () =>
      root.render(
        <RuntimeCategoryPicker current="native" value="native" canSwitch onChange={change} />,
      ),
    );
    expect(find("runtimeSwitch.web").disabled).toBe(true);
    expect(find("runtimeSwitch.managed").disabled).toBe(false);
    expect(host.textContent).toContain("runtimeSwitch.nativeWebUnavailable");
    await act(async () =>
      root.render(
        <RuntimeCategoryPicker current="managed" value="managed" canSwitch onChange={change} />,
      ),
    );
    expect(find("runtimeSwitch.web").disabled).toBe(false);
    expect(host.textContent).not.toContain("runtimeSwitch.nativeWebUnavailable");
    expect(host.textContent).not.toContain("runtimeSwitch.webNativeUnavailable");
    await act(async () => find("runtimeSwitch.web").click());
    expect(change).toHaveBeenCalledWith("web");
    await act(async () =>
      root.render(<RuntimeCategoryPicker current="web" value="web" canSwitch onChange={change} />),
    );
    expect(find("runtimeSwitch.native").disabled).toBe(true);
    expect(find("runtimeSwitch.managed").disabled).toBe(false);
    expect(host.textContent).toContain("runtimeSwitch.webNativeUnavailable");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
