import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { createTerminalOutputStreamWriter } from "./terminalOutputStreamWriter";

function setup() {
  const writes: Array<{ data: Uint8Array; replaying: boolean; parsed: () => void }> = [];
  const onText = vi.fn();
  const writer = createTerminalOutputStreamWriter({
    write: (data, replaying, parsed) => writes.push({ data, replaying, parsed }),
    onText,
  });
  return { onText, writer, writes };
}

describe("terminal output stream writer", () => {
  afterEach(() => vi.useRealTimers());

  it("preserves raw bytes and commits only after xterm parses them", () => {
    const { writer, writes } = setup();
    const parsed = vi.fn();

    writer.write(new Uint8Array([0xff, 0x61]), false, parsed);

    expect(Array.from(writes[0].data)).toEqual([0xff, 0x61]);
    expect(parsed).not.toHaveBeenCalled();
    writes[0].parsed();
    expect(parsed).toHaveBeenCalledOnce();
  });

  it("replaces a clear-scrollback sequence split across network frames", () => {
    const { writer, writes } = setup();
    const firstParsed = vi.fn();
    const secondParsed = vi.fn();

    writer.write(new Uint8Array([0x1b, 0x5b]), true, firstParsed);
    expect(writes).toHaveLength(0);
    writer.write(new Uint8Array([0x33, 0x4a, 0x78]), false, secondParsed);

    expect(writes).toHaveLength(1);
    expect(Array.from(writes[0].data)).toEqual([0x1b, 0x5b, 0x32, 0x4a, 0x78]);
    expect(writes[0].replaying).toBe(true);
    expect(firstParsed).not.toHaveBeenCalled();
    expect(secondParsed).not.toHaveBeenCalled();
    writes[0].parsed();
    expect(firstParsed).toHaveBeenCalledOnce();
    expect(secondParsed).toHaveBeenCalledOnce();
  });

  it("flushes an incomplete escape prefix without advancing it early", () => {
    const { writer, writes } = setup();
    const parsed = vi.fn();
    writer.write(new Uint8Array([0x1b, 0x5b, 0x33]), true, parsed);
    expect(writes).toHaveLength(0);

    writer.flush();
    expect(Array.from(writes[0].data)).toEqual([0x1b, 0x5b, 0x33]);
    expect(parsed).not.toHaveBeenCalled();
    writes[0].parsed();
    expect(parsed).toHaveBeenCalledOnce();
  });

  it("commits an incomplete escape prefix after an idle boundary", () => {
    vi.useFakeTimers();
    const { writer, writes } = setup();
    const parsed = vi.fn();

    writer.write(new Uint8Array([0x1b, 0x5b, 0x33]), true, parsed);
    expect(writes).toHaveLength(0);
    vi.advanceTimersByTime(50);

    expect(Array.from(writes[0].data)).toEqual([0x1b, 0x5b, 0x33]);
    expect(parsed).not.toHaveBeenCalled();
    writes[0].parsed();
    expect(parsed).toHaveBeenCalledOnce();
  });

  it("keeps text observation streaming across split UTF-8", () => {
    const { onText, writer, writes } = setup();
    const encoded = new TextEncoder().encode("你");
    writer.write(encoded.slice(0, 2), false);
    writer.write(encoded.slice(2), false);

    expect(onText).toHaveBeenCalledTimes(1);
    expect(onText).toHaveBeenCalledWith("你");
    for (const write of writes) write.parsed();
  });
});
