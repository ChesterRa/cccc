// @vitest-environment happy-dom
import {
  Archive,
  ArchiveRestore,
  Code,
  LayoutGrid,
  Maximize,
  MessageSquare,
  MessagesSquare,
  Paperclip,
  RotateCcw,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { readdirSync, readFileSync } from "node:fs";
import { join, sep } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vite-plus/test";
import type { ReactElement } from "react";
import * as Icons from "./Icons";
import {
  ConnectMonitorIcon,
  GroupArchiveIcon,
  GroupArchiveRestoreIcon,
  GroupDeleteIcon,
  LcdIcon,
  LocalSessionsIcon,
  SearchIcon,
  WorkGridIcon,
  WorkMessagesIcon,
} from "./Icons";

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return entry.name.endsWith(".tsx") && !entry.name.endsWith(".test.tsx") ? [path] : [];
  });
}

function svg(markup: string) {
  const host = document.createElement("div");
  host.innerHTML = markup;
  const node = host.querySelector("svg");
  if (!node) throw new Error("Expected an SVG");
  return node;
}

describe("LCD icon adaptation", () => {
  it.each([
    [Code, "code"],
    [Maximize, "maximize"],
    [Paperclip, "attachment"],
    [RotateCcw, "reload"],
    [Sparkles, "sparkles"],
    [X, "close"],
  ] as const)("keeps direct Lucide attributes and paths for %s", (Icon, glyph) => {
    const props = { size: 16, className: "h-4 w-4", "aria-hidden": true as const };
    const before = svg(renderToStaticMarkup(<Icon {...props} />));
    const after = svg(renderToStaticMarkup(<LcdIcon icon={Icon} glyph={glyph} {...props} />));
    expect(after.querySelectorAll(":scope > [data-lcd-glyph]")).toHaveLength(1);
    after.querySelector(":scope > [data-lcd-glyph]")?.remove();
    after.removeAttribute("data-lcd-icon");
    expect(after.outerHTML).toBe(before.outerHTML);
  });

  it("keeps the original SVG attributes and a single root while adding a pixel glyph", () => {
    const props = { size: 13, strokeWidth: 1.25, className: "hidden @min-[480px]/header:block" };
    const original = svg(renderToStaticMarkup(<Search absoluteStrokeWidth {...props} />));
    const adapted = svg(renderToStaticMarkup(<SearchIcon {...props} />));
    for (const name of ["width", "height", "viewBox", "class", "stroke-width", "aria-hidden"]) {
      expect(adapted.getAttribute(name)).toBe(original.getAttribute(name));
    }
    expect(adapted.querySelectorAll("svg")).toHaveLength(0);
    expect(adapted.querySelectorAll(":scope > [data-lcd-glyph]")).toHaveLength(1);
    expect(adapted.querySelectorAll(":scope > path, :scope > circle")).toHaveLength(
      original.querySelectorAll(":scope > path, :scope > circle").length,
    );
  });

  it.each([
    { "aria-label": "Search messages" },
    { "aria-hidden": false },
    { role: "img" },
    { title: "Search messages" },
  ])("preserves lucide's accessibility attributes for %o", (props) => {
    const original = svg(
      renderToStaticMarkup(<Search size={18} strokeWidth={1.5} absoluteStrokeWidth {...props} />),
    );
    const adapted = svg(renderToStaticMarkup(<SearchIcon {...props} />));
    for (const name of ["aria-hidden", "aria-label", "role", "title"]) {
      expect(adapted.getAttribute(name)).toBe(original.getAttribute(name));
    }
  });

  it("leaves explicitly supplied children and their original semantics alone", () => {
    const adapted = svg(
      renderToStaticMarkup(
        <SearchIcon>
          <title>Find</title>
          <path data-extra="" d="M1 1h2" />
        </SearchIcon>,
      ),
    );
    expect(adapted.getAttribute("aria-hidden")).toBeNull();
    expect(adapted.querySelector("title")?.textContent).toBe("Find");
    expect(adapted.querySelector("[data-extra]")).not.toBeNull();
    expect(adapted.querySelector("[data-lcd-glyph]")).toBeNull();
  });

  it("keeps direct lucide defaults for Connect icons", () => {
    const adapted = svg(renderToStaticMarkup(<ConnectMonitorIcon size={16} />));
    expect(adapted.getAttribute("width")).toBe("16");
    expect(adapted.getAttribute("stroke-width")).toBe("2");
    expect(adapted.getAttribute("aria-hidden")).toBe("true");
  });

  it("keeps the existing local session icon in classic style", () => {
    const props = { size: 18, className: "shrink-0" };
    const original = svg(renderToStaticMarkup(<MessagesSquare {...props} />));
    const adapted = svg(renderToStaticMarkup(<LocalSessionsIcon {...props} />));
    expect(adapted.querySelectorAll(":scope > [data-lcd-glyph]")).toHaveLength(1);
    adapted.querySelector(":scope > [data-lcd-glyph]")?.remove();
    adapted.removeAttribute("data-lcd-icon");
    expect(adapted.outerHTML).toBe(original.outerHTML);
  });

  it("gives every exported icon a pixel glyph", () => {
    const missing = Object.entries(Icons).flatMap(([name, Icon]) => {
      if (name === "LcdIcon") return [];
      const Component = Icon as (props: object) => ReactElement;
      const adapted = svg(renderToStaticMarkup(<Component />));
      return adapted.querySelectorAll(":scope > [data-lcd-glyph]").length === 1 ? [] : [name];
    });
    expect(missing).toEqual([]);
  });

  it("renders no Lucide icon directly, so each one keeps its pixel glyph", () => {
    const src = join(process.cwd(), "src");
    const direct = sourceFiles(src).flatMap((file) => {
      if (file.endsWith(`components${sep}Icons.tsx`)) return [];
      const text = readFileSync(file, "utf8");
      return [...text.matchAll(/import \{([^}]*)\} from "lucide-react"/g)]
        .flatMap(([, names]) => names.split(","))
        .map(
          (name) =>
            name
              .trim()
              .split(/\s+as\s+/)
              .pop() ?? "",
        )
        .filter((name) => /^[A-Z]\w*$/.test(name) && new RegExp(`<${name}[\\s/>]`).test(text))
        .map((name) => `${file.slice(src.length)}: <${name}>`);
    });
    expect(direct).toEqual([]);
  });

  it("preserves the compact view switch's original SVG attributes", () => {
    for (const [original, adapted] of [
      [
        <MessageSquare key="original-message" size={17} />,
        <WorkMessagesIcon key="adapted-message" size={17} />,
      ],
      [<LayoutGrid key="original-grid" size={17} />, <WorkGridIcon key="adapted-grid" size={17} />],
    ]) {
      const before = svg(renderToStaticMarkup(original));
      const after = svg(renderToStaticMarkup(adapted));
      for (const name of ["width", "height", "viewBox", "class", "stroke-width", "aria-hidden"]) {
        expect(after.getAttribute(name)).toBe(before.getAttribute(name));
      }
      expect(after.querySelector(":scope > [data-lcd-glyph]")).not.toBeNull();
    }
  });

  it("preserves the original Group menu icons in classic style", () => {
    for (const [original, adapted] of [
      [<Archive key="archive" size={15} />, <GroupArchiveIcon key="lcd-archive" size={15} />],
      [
        <ArchiveRestore key="restore" size={15} />,
        <GroupArchiveRestoreIcon key="lcd-restore" size={15} />,
      ],
      [<Trash2 key="delete" size={15} />, <GroupDeleteIcon key="lcd-delete" size={15} />],
    ]) {
      const before = svg(renderToStaticMarkup(original));
      const after = svg(renderToStaticMarkup(adapted));
      for (const name of ["width", "height", "viewBox", "class", "stroke-width", "aria-hidden"]) {
        expect(after.getAttribute(name)).toBe(before.getAttribute(name));
      }
      expect(after.querySelector(":scope > [data-lcd-glyph]")).not.toBeNull();
    }
  });
});
