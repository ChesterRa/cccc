// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ActorAvatarField } from "./ActorAvatarField";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("rejects an unsupported SVG instead of uploading a file that cannot be displayed", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onSelectFile = vi.fn();
  try {
    await act(async () =>
      root.render(
        <ActorAvatarField isDark={false} onSelectFile={onSelectFile} onReset={vi.fn()} />,
      ),
    );
    const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
    expect(input.accept).not.toContain("svg");
    const choose = async (file: File) => {
      Object.defineProperty(input, "files", { configurable: true, value: [file] });
      await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    };
    await choose(new File(["<svg/>"], "avatar.svg", { type: "image/svg+xml" }));
    expect(onSelectFile).toHaveBeenCalledWith(null);
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("avatarUnsupportedFormat");
    const png = new File(["png"], "avatar.png", { type: "image/png" });
    await choose(png);
    expect(onSelectFile).toHaveBeenCalledWith(png);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
