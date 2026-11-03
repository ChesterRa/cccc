import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vite-plus/test";

type Tree = { [key: string]: string | Tree };

function keysOf(tree: Tree, prefix = ""): string[] {
  return Object.entries(tree).flatMap(([key, value]) => {
    const path = `${prefix}${key}`;
    return typeof value === "string" ? [path] : [path, ...keysOf(value, `${path}.`)];
  });
}

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

describe("locale keys", () => {
  it("defines in English every key the Web passes to t() as a literal", () => {
    // A defaultValue is no translation: without the key, Chinese and Japanese readers see the
    // English fallback, or the key itself. Chinese and Japanese follow English through the
    // repository's locale parity test.
    //
    // The scan reads source text, not syntax: it skips whole comment lines but would still see a
    // t("…") in a trailing comment or inside a string, and it does not see keys built at run time.
    const english = join(process.cwd(), "src", "i18n", "locales", "en");
    const namespaces = new Map(
      readdirSync(english)
        .filter((name) => name.endsWith(".json"))
        .map((name) => {
          const tree = JSON.parse(readFileSync(join(english, name), "utf8")) as Tree;
          return [name.replace(/\.json$/, ""), new Set(keysOf(tree))] as const;
        }),
    );
    // A prefixed key must be in its namespace; a bare one in at least one, since a component can
    // take its t from a parent. So this does not show that a bare key is in the namespace its t uses.
    const defined = (namespace: string | null, key: string) =>
      (namespace ? [namespaces.get(namespace)] : [...namespaces.values()]).some((keys) =>
        ["", "_one", "_other"].some((suffix) => keys?.has(key + suffix)),
      );
    const src = join(process.cwd(), "src");
    const missing = sourceFiles(src).flatMap((file) => {
      const text = readFileSync(file, "utf8")
        .split("\n")
        .filter((line) => !/^\s*(\/\/|\/\*|\*)/.test(line))
        .join("\n");
      return [...text.matchAll(/(?<![\w$])t\(\s*(["'])([A-Za-z][\w.:-]*)\1\s*[,)]/g)].flatMap(
        ([, , literal]) => {
          const at = literal.indexOf(":");
          const namespace = at < 0 ? null : literal.slice(0, at);
          return defined(namespace, literal.slice(at + 1))
            ? []
            : [`${relative(src, file)}: ${literal}`];
        },
      );
    });
    expect(missing).toEqual([]);
  });
});
