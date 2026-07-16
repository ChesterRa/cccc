import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { ImagePreview } from "./ImagePreview";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("ImagePreview fixed layout", () => {
  it.each([
    ["hero", 224],
    ["grid", 128],
  ] as const)("keeps the %s preview at %dpx before image decode", (layout, height) => {
    const markup = renderToStaticMarkup(
      <ImagePreview
        href="/api/v1/groups/g1/blobs/image"
        alt="attachment"
        isSvg={false}
        isUserMessage={false}
        isDark={false}
        layout={layout}
      />,
    );

    expect(markup).toContain(`height:${height}px`);
    expect(markup).toContain("object-contain");
    expect(markup).not.toContain("object-cover");
  });
});
