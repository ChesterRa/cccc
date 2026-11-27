// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useGroupStore } from "../../stores";
import type { RuntimeInfo } from "../../types";
import { RuntimeSelector } from "./RuntimeSelector";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  Trans: ({ i18nKey }: { i18nKey: string }) => i18nKey,
}));

describe("RuntimeSelector status line", () => {
  const host = document.createElement("div");
  let root: Root;
  const previous = useGroupStore.getState();
  const show = async (available: boolean, quietWhenDetected: boolean) => {
    useGroupStore.setState({
      runtimes: [{ name: "codex", available } as RuntimeInfo],
      runtimeDetectionStatus: "ready",
    });
    await act(async () =>
      root.render(
        <RuntimeSelector
          value="codex"
          onChange={() => undefined}
          showHint={false}
          quietWhenDetected={quietWhenDetected}
        />,
      ),
    );
  };
  const status = () => host.querySelector('[role="status"]')?.textContent;
  const recheck = () =>
    [...host.querySelectorAll("button")].find((node) =>
      node.textContent?.includes("runtimeDetection.recheck"),
    );

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useGroupStore.setState(previous, true);
  });

  it("says nothing about a runtime that was found when asked to stay quiet", async () => {
    await show(true, true);

    expect(status()).toBeUndefined();
    expect(recheck()).toBeUndefined();
  });

  it("still warns about a missing runtime and offers to check again", async () => {
    await show(false, true);

    expect(status()).toBe("runtimeDetection.missing");
    expect(recheck()).toBeDefined();
  });

  it("offers no recheck for a Web Model runtime, which has no local CLI to find", async () => {
    useGroupStore.setState({ runtimes: [], runtimeDetectionStatus: "ready" });
    await act(async () =>
      root.render(
        <RuntimeSelector
          value="grok_web_model"
          onChange={() => undefined}
          showHint={false}
          quietWhenDetected
        />,
      ),
    );

    expect(status()).toBeUndefined();
    expect(recheck()).toBeUndefined();
  });

  it("reports a found runtime by default", async () => {
    await show(true, false);

    expect(status()).toBe("runtimeDetection.detected");
    expect(recheck()).toBeDefined();
  });
});
