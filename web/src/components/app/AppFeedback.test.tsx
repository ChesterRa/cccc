import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FeedbackText } from "./AppFeedback";

describe("FeedbackText", () => {
  it("keeps each word of a quoted command whole", () => {
    const markup = renderToStaticMarkup(
      <FeedbackText text="Run `cccc setup --runtime deepseek` first." />,
    );
    expect(markup).toBe(
      'Run <code><span class="inline-block max-w-full [overflow-wrap:anywhere]">cccc</span> <span class="inline-block max-w-full [overflow-wrap:anywhere]">setup</span> <span class="inline-block max-w-full [overflow-wrap:anywhere]">--runtime</span> <span class="inline-block max-w-full [overflow-wrap:anywhere]">deepseek</span></code> first.',
    );
  });

  it("leaves text without a closed pair of backticks as written", () => {
    expect(renderToStaticMarkup(<FeedbackText text="plain error" />)).toBe("plain error");
    expect(renderToStaticMarkup(<FeedbackText text="a `b` and `c" />)).toBe(
      'a <code><span class="inline-block max-w-full [overflow-wrap:anywhere]">b</span></code> and `c',
    );
  });
});
