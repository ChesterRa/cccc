// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";

// Each branch can unmount everything under it: a sign-in prompt, or an expired Connect frame.
const shell = vi.hoisted(() => ({ open: true }));
vi.mock("./App", () => ({ default: () => <div data-app /> }));
vi.mock("./components/AuthGate", () => ({
  AuthGate: ({ children }: { children: ReactNode }) =>
    shell.open ? children : <form data-login />,
}));
vi.mock("./features/connect/ConnectEmbeddedApp", () => ({
  ConnectEmbeddedApp: () => (shell.open ? <div data-app /> : <p data-reopen />),
}));
vi.mock("./components/capabilities/CapabilityCenterStandaloneApp", () => ({
  CapabilityCenterStandaloneApp: () => null,
}));

import { WebRoot } from "./WebRoot";
import { useComposerStore } from "./stores/useComposerStore";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function leaveIsBlocked() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

it.each(["/ui/", "/ui/connect"])(
  "keeps warning before a reload while sends wait after %s unmounts the app",
  async (pathname) => {
    shell.open = true;
    useComposerStore.setState({
      failedSendsByGroup: {
        "g-a": [
          {
            id: "local-1",
            originGroupId: "g-a",
            destGroupId: "g-a",
            retryKey: null,
            composerText: "waiting",
            composerFiles: [],
            toText: "",
            replyTarget: null,
            quotedPresentationRef: null,
            quotedVoiceDocumentRef: null,
            messageMode: "send",
          },
        ],
      },
    });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<WebRoot pathname={pathname} />));
    expect(host.querySelector("[data-app]")).not.toBeNull();
    expect(leaveIsBlocked()).toBe(true);

    shell.open = false;
    await act(async () => root.render(<WebRoot pathname={pathname} />));
    expect(host.querySelector("[data-app]")).toBeNull();
    expect(leaveIsBlocked()).toBe(true);

    act(() => useComposerStore.setState({ failedSendsByGroup: {} }));
    expect(leaveIsBlocked()).toBe(false);
    await act(async () => root.unmount());
    host.remove();
  },
);
