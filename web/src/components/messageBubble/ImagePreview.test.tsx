import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { ImagePreview } from "./ImagePreview";
import { ImagePreviewFailure } from "./ImagePreviewFailure";

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

  it("keeps the grid failure state inside the fixed preview height", () => {
    const markup = renderToStaticMarkup(
      <ImagePreviewFailure
        href="/api/v1/groups/g1/blobs/missing-image"
        alt="a-very-long-image-name-that-must-not-overlap.png"
        isUserMessage={false}
        isDark={false}
        layout="grid"
        height={128}
        title="Download image"
        unavailableLabel="Image preview unavailable"
        openOriginalLabel="Open the original image"
      />,
    );

    expect(markup).toContain("height:128px");
    expect(markup).toContain("justify-center overflow-hidden rounded-lg");
    expect(markup).toContain("line-clamp-2 break-words text-[11px] leading-4");
    expect(markup).toContain("mt-0.5 truncate text-[10px] leading-4");
    expect(markup).toContain('width="20"');
    expect(markup).toContain("mb-1");
    expect(markup).toContain("pt-1");
  });
});
