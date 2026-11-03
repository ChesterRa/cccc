import { readFileSync } from "node:fs";
import { Window } from "happy-dom";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vite-plus/test";
import { ChetHead } from "./ChetHead";

const motionCss = readFileSync(new URL("../../styles/lcdMotion.css", import.meta.url), "utf8");

/** The animation each frame gets and whether it shows, as computed from the stylesheet. */
async function frameStyles(working: boolean, motion: "on" | "off") {
  // A fresh window for each case: happy-dom keeps computed styles once they are read.
  const window = new Window();
  const { document } = window;
  document.documentElement.setAttribute("data-ui-style", "lcd");
  if (motion === "off") document.documentElement.setAttribute("data-lcd-motion", "off");
  const style = document.createElement("style");
  style.textContent = motionCss;
  document.head.append(style);
  document.body.innerHTML = renderToStaticMarkup(<ChetHead size={32} working={working} />);
  const styles = Object.fromEntries(
    [...document.querySelectorAll("g[data-frame]")].map((frame) => {
      const computed = window.getComputedStyle(frame);
      return [
        frame.getAttribute("data-frame") ?? "",
        { animation: computed.animation, shown: computed.opacity !== "0" },
      ];
    }),
  );
  await window.happyDOM.close();
  return styles;
}

/** Each frame redrawn from its path as rows of "#" (ink) and "." so a test can show the picture. */
function frames(html: string, grid: number) {
  return [...html.matchAll(/<g data-frame="([a-z]+)">(.*?)<\/g>/g)].map(([, name, body]) => {
    const rows = Array.from({ length: grid }, () => Array<string>(grid).fill("."));
    for (const [, x, y, w] of body.matchAll(/M(\d+) (\d+)h(\d+)/g)) {
      for (let i = 0; i < Number(w); i += 1) rows[Number(y)][Number(x) + i] = "#";
    }
    return { name, rows: rows.map((row) => row.join("")) };
  });
}

describe("ChetHead", () => {
  it("draws a disc with two eyes on the 16 grid, hidden from assistive technology", () => {
    const html = renderToStaticMarkup(<ChetHead size={16} />);
    expect(html).toContain('viewBox="0 0 16 16"');
    expect(html).toContain('width="16"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain('shape-rendering="crispEdges"');
    const [rest, blink, work, glance] = frames(html, 16);
    expect([rest, blink, work, glance].map((frame) => frame.name)).toEqual([
      "rest",
      "blink",
      "work",
      "glance",
    ]);
    expect(rest.rows).toEqual([
      ".....######.....",
      "...##########...",
      "..############..",
      ".##############.",
      ".##############.",
      "################",
      "####..####..####",
      "####..####..####",
      "################",
      "################",
      "################",
      ".##############.",
      ".##############.",
      "..############..",
      "...##########...",
      ".....######.....",
    ]);
    // A blink closes each eye to one row; at work the eyes glance one cell left, then right.
    expect(blink.rows.slice(6, 8)).toEqual(["################", "####..####..####"]);
    expect(work.rows.slice(6, 8)).toEqual(["###..####..#####", "###..####..#####"]);
    expect(glance.rows.slice(6, 8)).toEqual(["#####..####..###", "#####..####..###"]);
  });

  it("has its own 32 grid: the same disc with a finer edge, symmetric", () => {
    const html = renderToStaticMarkup(<ChetHead size={32} />);
    expect(html).toContain('viewBox="0 0 32 32"');
    expect(html).toContain('width="32"');
    const [rest, blink] = frames(html, 32);
    for (const row of rest.rows) expect(row).toBe([...row].reverse().join(""));
    // Filled in, the eyes aside, the outline reads the same from the top and from the bottom.
    const outline = rest.rows.map((row) => row.replace(/#\.+#/g, (gap) => "#".repeat(gap.length)));
    expect(outline).toEqual([...outline].reverse());
    expect(rest.rows.slice(0, 3)).toEqual([
      "............########............",
      ".........##############.........",
      ".......##################.......",
    ]);
    expect(rest.rows.slice(11, 15)).toEqual([
      ".#######....########....#######.",
      "########....########....########",
      "########....########....########",
      "########....########....########",
    ]);
    // Closed, each eye is a single row: one row has a gap between ink cells.
    expect(blink.rows.filter((row) => /#\.+#/.test(row))).toEqual([
      "########....########....########",
    ]);
  });

  it("names its state for the motion rules", () => {
    expect(renderToStaticMarkup(<ChetHead size={16} />)).toContain('data-chet-head="idle"');
    expect(renderToStaticMarkup(<ChetHead size={16} working />)).toContain(
      'data-chet-head="working"',
    );
  });

  it("moves in whole steps and holds the face of its state when motion is off", async () => {
    const idle = await frameStyles(false, "on");
    expect(idle.rest.animation).toBe("chet-head-awake 6s steps(1, end) infinite");
    expect(idle.blink.animation).toBe("chet-head-blink 6s steps(1, end) infinite");
    expect([idle.work.shown, idle.glance.shown]).toEqual([false, false]);
    const working = await frameStyles(true, "on");
    expect(working.work.animation).toBe("chet-head-first-half 1.6s steps(1, end) infinite");
    expect(working.glance.animation).toBe("chet-head-second-half 1.6s steps(1, end) infinite");
    expect([working.rest.shown, working.blink.shown]).toEqual([false, false]);
    // With motion off nothing moves, and each state holds its own face.
    for (const [isWorking, face] of [
      [false, "rest"],
      [true, "work"],
    ] as const) {
      const still = await frameStyles(isWorking, "off");
      for (const [name, frame] of Object.entries(still)) {
        expect(frame.animation, name).toBe("none");
        expect(frame.shown, name).toBe(name === face);
      }
    }
  });
});
