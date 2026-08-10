// @vitest-environment happy-dom

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { GroupStatusIndicator } from "../../src/components/layout/GroupStatusIndicator";
import { IconButton } from "../../src/components/ui/icon-button";
import { getGroupStatusUnified } from "../../src/utils/groupStatus";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        statusRunning: "运行中",
        statusPaused: "已暂停",
        statusIdle: "待机",
        statusStopped: "已停止",
      })[key] || key,
  }),
}));

describe("shared UI primitives", () => {
  it("gives icon-only buttons one consistent accessible name", () => {
    const html = renderToStaticMarkup(
      <IconButton label="打开设置">
        <span aria-hidden="true">S</span>
      </IconButton>,
    );

    expect(html).toContain('aria-label="打开设置"');
    expect(html).toContain('title="打开设置"');
  });

  it("shows a localized group status instead of an internal status code", () => {
    const html = renderToStaticMarkup(
      <GroupStatusIndicator status={getGroupStatusUnified(true, "active")} variant="badge" />,
    );

    expect(html).toContain("运行中");
    expect(html).not.toContain(">RUN<");
  });
});
