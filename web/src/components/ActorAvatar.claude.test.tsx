// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vite-plus/test";
import { ActorAvatar } from "./ActorAvatar";

describe("Claude actor avatar", () => {
  it("uses the inline logo and returns to it if a custom avatar fails", async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<ActorAvatar runtime="claude" isDark={false} />));
      expect(host.querySelector('svg[viewBox="0 0 16 16"] path')).not.toBeNull();
      expect(host.querySelector("img")).toBeNull();

      await act(async () =>
        root.render(<ActorAvatar runtime="claude" avatarUrl="/avatar.png" isDark={false} />),
      );
      expect(host.querySelector("img")).not.toBeNull();
      expect(host.querySelector("svg")).toBeNull();
      await act(async () => host.querySelector("img")!.dispatchEvent(new Event("error")));
      expect(host.querySelector('svg[viewBox="0 0 16 16"] path')).not.toBeNull();
      expect(host.querySelector("img")).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
