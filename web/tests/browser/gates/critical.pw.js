import { test, expect, groupPage, wheelTo } from "./helpers.js";

test("connection feedback preserves the draft and clears after reconnection", async ({ page }) => {
  await groupPage(page);
  const input = page.getByRole("textbox", { name: "Message input" });
  await input.fill("Keep this draft");
  await page.evaluate("groupWorkProbe.setConnectionStatus('disconnected')");
  await expect(page.locator("header [role=status]:visible")).toHaveText("Disconnected");
  await expect(input).toBeFocused();
  await page.evaluate("groupWorkProbe.setConnectionStatus('connecting')");
  await expect(page.locator("header [role=status]:visible")).toHaveText("Reconnecting…");
  await page.evaluate("groupWorkProbe.setConnectionStatus('connected')");
  await expect(page.locator("header [role=status]:visible")).toHaveCount(0);
  await expect(input).toHaveValue("Keep this draft");
  await expect(input).toBeFocused();
});

test("an interrupted send keeps its draft and reports an unconfirmed result without retry", async ({
  page,
}) => {
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.interruptedSends = 0;
    window.fetch = async (...args) => {
      if (String(args[0]).endsWith("/send")) {
        interruptedSends++;
        throw new TypeError("Network response was interrupted");
      }
      return original(...args);
    };
  });
  const input = page.getByRole("textbox", { name: "Message input" });
  await input.fill("Keep uncertain delivery");
  await input.press("Control+Enter");
  await expect(input).toHaveValue("Keep uncertain delivery");
  await expect.poll(() => page.evaluate("interruptedSends")).toBe(1);
  await expect
    .poll(() => page.evaluate("groupWorkProbe.ui.getState().errorMsg"))
    .toContain("Delivery could not be confirmed");
  expect(await page.evaluate("interruptedSends")).toBe(1);
});

test("same-version build differences are visible and can be copied without credentials", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.fetch = async (...args) =>
      String(args[0]).includes("/api/v1/ping")
        ? Response.json({
            ok: true,
            result: {
              version: "0.4.40",
              build: { source_id: "web-source" },
              daemon: { version: "0.4.40", build: { source_id: "daemon-source" } },
              web: { assets_id: "bundle", entry_script: "/ui/assets/entry.js" },
            },
          })
        : original(...args);
    groupWorkProbe.openSettings("global", "developer");
  });
  await expect(page.getByText("web-source", { exact: true })).toBeVisible();
  await expect(page.getByText(/These components use different builds/)).toBeVisible();
  await page.getByRole("button", { name: "Copy build information" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByRole("status")).toHaveText(
    "Copied",
  );
  const copied = JSON.parse(await page.evaluate("navigator.clipboard.readText()"));
  expect(copied.webSource).toBe("web-source");
  expect(copied.daemonSource).toBe("daemon-source");
  expect(Object.keys(copied).sort()).toEqual(
    [
      "daemonSource",
      "daemon_version",
      "loadedEntry",
      "servedEntry",
      "webAssets",
      "webSource",
      "web_version",
    ].sort(),
  );
  await test
    .info()
    .attach("build-diagnostics", { body: await page.screenshot(), contentType: "image/png" });
});

test("remote references survive Group switching and a late Voice draft update", async ({
  page,
}) => {
  await groupPage(page);
  const composer = page.locator("textarea");
  await composer.fill("#");
  await page.getByRole("option").filter({ hasText: "Mac Studio" }).click();
  await composer.press("End");
  await composer.pressSequentially("@");
  await page.getByRole("option").filter({ hasText: "Remote worker" }).click();
  await page.evaluate("groupWorkProbe.chooseGroup('g2')");
  await expect(composer).toHaveValue("");
  await page.evaluate(async () => {
    const { routeVoiceTextToComposerGroup } =
      await import("/ui/src/pages/chat/voice-secretary/voiceComposerDraftRouting.ts");
    routeVoiceTextToComposerGroup({ groupId: "g1", text: "voice update", mode: "append" });
  });
  await expect(composer).toHaveValue("");
  await page.evaluate("groupWorkProbe.chooseGroup('g1')");
  await expect(composer).toHaveValue(/voice update$/);
  await composer.press("Control+Enter");
  await expect
    .poll(() => page.evaluate("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).length"))
    .toBe(1);
  const sent = await page.evaluate("groupWorkProbe.requests.find(r=>r.path.endsWith('/send'))");
  expect(sent.path).toBe("/api/v1/groups/g1/send");
  expect(sent.body.refs[0].instance_id).toBe("i_mac");
  expect(sent.body.text).toContain("voice update");
  expect(sent.body.dst_instance_id).toBeUndefined();
  await expect(composer).toHaveValue("");
});

test("retained terminals preserve their view and resynchronize a returning writer", async ({
  page,
}) => {
  await groupPage(page);
  await page.getByRole("button", { name: "Terminals", exact: true }).click();
  const first = page.locator(
    '[data-runtime-group-id="g1"][data-runtime-actor-id="actor-1"] .xterm-screen',
  );
  await expect(first).toBeVisible();
  await expect
    .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
    .toBe(4);
  await page.evaluate(async () => {
    const p = window.groupWorkProbe;
    window.keptTerm = p.terminals.find((t) =>
      t.element?.closest('[data-runtime-actor-id="actor-1"]'),
    );
    window.keptSocket = p.sockets.find((s) => s.readyState === 1 && s.actor === "actor-1");
    window.keptScreen = keptTerm.element.querySelector(".xterm-screen");
    await new Promise((done) =>
      keptTerm.write(Array.from({ length: 200 }, (_, i) => `cache line ${i}\r\n`).join(""), done),
    );
    keptTerm.scrollToLine(30);
    keptTerm.select(0, 32, 8);
    window.keptScroll = keptTerm.buffer.active.viewportY;
    window.keptSelection = keptTerm.getSelection();
    window.keptSize = { cols: keptTerm.cols, rows: keptTerm.rows };
  });
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(first).toBeHidden();
  await expect
    .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
    .toBe(8);
  const resizes = await page.evaluate("keptSocket.frames.filter(f=>f.type===50).length");
  await page.evaluate(() => {
    for (const terminal_writable of [false, true]) {
      keptSocket.onmessage({
        data: new TextEncoder().encode("6" + JSON.stringify({ terminal_writable })).buffer,
      });
    }
    groupWorkProbe.chooseGroup("g2");
  });
  await expect(page.getByRole("textbox", { name: "Message input" })).toBeVisible();
  expect(await page.evaluate("keptSocket.frames.filter(f=>f.type===50).length")).toBe(resizes);
  await page.evaluate("groupWorkProbe.chooseGroup('g1')");
  await page.getByRole("button", { name: "Previous page", exact: true }).click();
  await expect(first).toBeVisible();
  await expect
    .poll(() => page.evaluate("keptSocket.frames.filter(f=>f.type===50).length"))
    .toBe(resizes + 1);
  expect(
    await page.evaluate("JSON.parse(keptSocket.frames.filter(f=>f.type===50).at(-1).text)"),
  ).toEqual(await page.evaluate("keptSize"));
  expect(
    await page.evaluate(
      "keptTerm.element.querySelector('.xterm-screen')===keptScreen && keptTerm.buffer.active.viewportY===keptScroll && keptTerm.getSelection()===keptSelection",
    ),
  ).toBe(true);
  expect(await page.evaluate("groupWorkProbe.sockets.length")).toBe(8);
});

test("PDF automatic refresh keeps the same iframe and URL", async ({ page }) => {
  await groupPage(page);
  // Native PDF rendering is browser-owned. Observe its actual iframe lifecycle;
  // the asset itself stays local and does not require a daemon or external URL.
  await page.route("**/presentation/slots/**", (route) =>
    route.fulfill({ contentType: "application/pdf", body: "%PDF-1.4\n%%EOF" }),
  );
  await page.evaluate(() => {
    groupWorkProbe.group.setState({
      groupPresentation: {
        v: 1,
        slots: [
          {
            slot_id: "slot-1",
            index: 1,
            card: {
              slot_id: "slot-1",
              title: "Report",
              card_type: "pdf",
              published_at: "2026-09-18T00:00:00Z",
              published_by: "worker",
              content: { mode: "workspace_link", workspace_rel_path: "report.pdf" },
            },
          },
        ],
      },
    });
    groupWorkProbe.modals
      .getState()
      .setPresentationViewer({ groupId: "g1", slotId: "slot-1", surface: "modal" });
  });
  const iframe = page.locator('[role="dialog"] iframe');
  await expect(iframe).toBeVisible();
  await page.evaluate(() => {
    window.keptPdf = document.querySelector('[role="dialog"] iframe');
    window.pdfSource = keptPdf.src;
    window.pdfMutations = 0;
    new MutationObserver(() => window.pdfMutations++).observe(keptPdf, {
      attributes: true,
      attributeFilter: ["src"],
    });
  });
  await page.clock.install();
  await page.clock.runFor(16_000);
  expect(
    await page.evaluate(
      "document.querySelector('[role=dialog] iframe')===keptPdf && keptPdf.src===pdfSource && pdfMutations===0",
    ),
  ).toBe(true);
});

test("Web Access refresh preserves the complete draft through Save", async ({ page }) => {
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.accessSaves = [];
    let access = {
      provider: "off",
      mode: "tailnet_only",
      require_access_token: true,
      enabled: false,
      status: "stopped",
      config: { web_host: "127.0.0.1", web_port: 8848, web_public_url: "" },
    };
    window.fetch = async (...args) => {
      const path = new URL(String(args[0]), location.href).pathname;
      if (path.endsWith("/access-tokens"))
        return Response.json({
          ok: true,
          result: {
            access_tokens: [
              { token_id: "fixture-admin", user_id: "owner", is_admin: true, allowed_groups: [] },
            ],
          },
        });
      if (path.endsWith("/remote_access")) {
        if (args[1]?.method === "PUT") {
          const draft = JSON.parse(args[1].body);
          accessSaves.push(draft);
          access = {
            ...access,
            ...draft,
            config: {
              web_host: draft.web_host,
              web_port: draft.web_port,
              web_public_url: draft.web_public_url,
            },
          };
        }
        return Response.json({ ok: true, result: { remote_access: access } });
      }
      return original(...args);
    };
    groupWorkProbe.openSettings("global", "webAccess");
  });
  await page.getByRole("button", { name: /Private network/ }).click();
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect(page.getByRole("button", { name: "Refresh", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect.poll(() => page.evaluate("accessSaves.length")).toBe(1);
  expect(await page.evaluate("accessSaves[0]")).toMatchObject({
    provider: "manual",
    web_host: "0.0.0.0",
    require_access_token: true,
  });
  await expect(page.getByRole("button", { name: "Saved", exact: true })).toBeVisible();
});

for (const mode of ["instruction", "prompt"]) {
  test(`Voice ${mode} opens linked documents without changing the recording target`, async ({
    page,
  }) => {
    await page.goto(`/ui/tests/browser/voice-workspace-mobile.html?mode=${mode}&lang=en`);
    const link = page
      .locator('[data-voice-activity-item="first"] [data-voice-document-link]')
      .last();
    await expect(link).toBeVisible();
    await page.locator("[data-voice-record]").click();
    await expect.poll(() => page.evaluate("voiceWorkspaceProbe.starts")).toBe(1);
    const writes = await page.evaluate("voiceWorkspaceProbe.writes.length");
    await link.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-voice-document-title]")).toHaveText(
      "Linked activity document",
    );
    await expect(page.locator("[data-voice-back-to-activity]")).toBeFocused();
    await expect(page.locator("[data-voice-mobile-sheet]")).toHaveAttribute(
      "data-voice-sheet-mode",
      mode,
    );
    expect(await page.evaluate("voiceWorkspaceProbe.stops")).toBe(0);
    expect(await page.evaluate("voiceWorkspaceProbe.writes.length")).toBe(writes);
    await page.keyboard.press("Enter");
    await expect(link).toBeFocused();
    await page.locator("[data-voice-record]").click();
    await expect.poll(() => page.evaluate("voiceWorkspaceProbe.stops")).toBe(1);
  });
}

for (const [width, height, lang, scale] of [
  [320, 568, "ja", 125],
  [390, 667, "en", 100],
]) {
  test(`short Prompt controls remain reachable at ${width}x${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.goto(
      `/ui/tests/browser/voice-workspace-mobile.html?mode=prompt&lang=${lang}&scale=${scale}&extra=15`,
    );
    await page.locator(".voice-mobile-prompt-toggle").click();
    const point = await wheelTo(
      page,
      page.locator("[data-voice-workspace-optimize]"),
      page.locator("[data-voice-request-panel]"),
    );
    await page.mouse.click(point.x, point.y);
    await expect
      .poll(() =>
        page.evaluate("voiceWorkspaceProbe.writes.some(r=>r.body.kind==='prompt_refine')"),
      )
      .toBe(true);
    const link = page
      .locator('[data-voice-activity-item="first"] [data-voice-document-link]')
      .last();
    const linkPoint = await wheelTo(page, link);
    await page.mouse.click(linkPoint.x, linkPoint.y);
    await expect(page.locator("[data-voice-document-title]")).toHaveText(
      "Linked activity document",
    );
    await page.locator("[data-voice-back-to-activity]").click();
    const area = page.locator("[data-voice-activity-scroll]");
    await area.hover();
    await page.mouse.wheel(0, 300);
    await expect.poll(() => area.evaluate((e) => e.scrollTop)).toBeGreaterThan(0);
  });
}

test("Group menus preserve navigation and block duplicate lifecycle requests", async ({ page }) => {
  await groupPage(page);
  await page.evaluate(() => {
    groupWorkProbe.setRunTransport(500);
  });
  const trigger = page.locator(
    'aside button[aria-haspopup="menu"][aria-label$="Research workspace"]',
  );
  await trigger.focus();
  await trigger.click();
  await page.getByRole("menuitem", { name: "Stop Group", exact: true }).click();
  await page.locator("header [data-group-run-controls]").click();
  await expect(
    page.getByRole("menuitem", { name: "Pause message delivery", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.evaluate(() => groupWorkProbe.chooseGroup("g2"));
  await expect.poll(() => page.evaluate(() => groupWorkProbe.ui.getState().busy)).toBe("");
  await expect(page.locator("header [data-group-run-controls]")).toHaveText("Stopped");
  expect(await page.evaluate(() => groupWorkProbe.group.getState().groupDoc.group_id)).toBe("g2");
});

test("message filters remain visible and mobile retains its status control", async ({ page }) => {
  await groupPage(page);
  const filters = page.locator("[data-message-filters]");
  const filter = filters.getByRole("button", { name: "All", exact: true });
  await expect(filter).toBeVisible();
  await expect(filters.getByRole("group")).toHaveCSS("opacity", "1");
  expect(
    await filters.evaluate(
      (e) =>
        e.getBoundingClientRect().bottom <=
        document.querySelector('[role="log"]').getBoundingClientRect().top + 1,
    ),
  ).toBe(true);
  await page.setViewportSize({ width: 390, height: 667 });
  const status = page.locator("header [data-group-run-controls]");
  await expect(status).toBeVisible();
  await status.click();
  await expect(
    page.getByRole("menuitem", { name: "Pause message delivery", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(status).toBeFocused();
});

for (const surface of ["files", "presentation"]) {
  test(`header aligns with restored ${surface} on first load and Group changes`, async ({
    page,
  }) => {
    await groupPage(page);
    await page
      .locator(
        surface === "files" ? "[data-workspace-files-toggle]" : "[data-group-presentation-trigger]",
      )
      .click();
    await page.evaluate(() =>
      groupWorkProbe.ui.getState().setChatSidePanelLayout("g1", { compact: false }),
    );
    await page.reload();
    const checkAlignment = async () => {
      await expect(page.locator("#group-side-panel")).toBeVisible();
      await expect
        .poll(() =>
          page.evaluate(() => {
            const panel = document.querySelector("#group-side-panel").getBoundingClientRect();
            const work = document.querySelector("[data-group-header-work]").getBoundingClientRect();
            return Math.abs(work.right - (panel.left - 4));
          }),
        )
        .toBeLessThanOrEqual(2);
    };
    await checkAlignment();
    await page.evaluate(() => groupWorkProbe.chooseGroup("g2"));
    await expect(page.locator("#group-side-panel")).toHaveCount(0);
    await page.evaluate(() => groupWorkProbe.chooseGroup("g1"));
    await checkAlignment();
    await page.setViewportSize({ width: 390, height: 667 });
    await expect(page.locator("#group-side-panel")).not.toBeVisible();
    await page.setViewportSize({ width: 1440, height: 1000 });
    await checkAlignment();
    if (surface === "presentation") {
      await page.evaluate(() =>
        groupWorkProbe.ui.getState().setChatSidePanelLayout("g1", { compact: true }),
      );
      await page.reload();
      await expect(page.locator("#group-side-panel")).toHaveCSS("width", "64px");
      await expect
        .poll(() =>
          page
            .locator("[data-group-shell]")
            .evaluate((e) => e.style.getPropertyValue("--group-side-panel-width")),
        )
        .toBe("64px");
    }
  });
}

test("header work tools follow panel resizing and stay reachable", async ({ page }) => {
  await groupPage(page);
  await page.evaluate(() => groupWorkProbe.ui.getState().setGroupWorkView("g1", "terminals"));
  await expect(page.locator(".xterm").first()).toBeVisible();
  await page.locator("[data-workspace-files-toggle]").click();
  const checkAlignment = async () => {
    await expect
      .poll(() =>
        page.evaluate(() => {
          const panel = document.querySelector("#group-side-panel").getBoundingClientRect();
          const work = document.querySelector("[data-group-header-work]").getBoundingClientRect();
          return Math.abs(work.right - (panel.left - 4));
        }),
      )
      .toBeLessThanOrEqual(2);
  };
  await checkAlignment();
  const divider = page.locator("[data-side-panel-resize]");
  await divider.focus();
  await divider.press("ArrowLeft");
  await checkAlignment();
  await page.evaluate(() =>
    groupWorkProbe.ui.getState().setChatSidePanelLayout("g1", { width: 700 }),
  );
  await checkAlignment();
  for (const label of ["Search messages", "Context Panel"]) {
    await expect(page.getByRole("button", { name: label, exact: true })).toBeVisible();
  }
  const buttons = page.locator("header button:visible");
  expect(
    await buttons.evaluateAll((items) =>
      items.every((item, i) => {
        const r = item.getBoundingClientRect();
        return (
          r.left >= 0 &&
          r.right <= innerWidth &&
          (!i || items[i - 1].getBoundingClientRect().right <= r.left + 1)
        );
      }),
    ),
  ).toBe(true);
  await page.locator("[data-workspace-files-toggle]").click();
  await expect(page.locator("#group-side-panel")).toHaveCount(0);
  expect(
    await page
      .locator("[data-group-shell]")
      .evaluate((e) => e.style.getPropertyValue("--group-side-panel-width")),
  ).toBe("");
  await page.locator("[data-group-presentation-trigger]").click();
  await expect(page.locator("[data-group-presentation-trigger]")).toHaveAttribute(
    "aria-expanded",
    "true",
  );
  expect(await page.locator("[data-group-presentation-trigger]").innerText()).toBe("");
});

test("appearance labels scale with text preferences and remain readable in both themes", async ({
  page,
}) => {
  await groupPage(page);
  for (const dark of [false, true]) {
    await page.evaluate((dark) => {
      groupWorkProbe.setDark(dark);
      groupWorkProbe.setTextScale(125);
    }, dark);
    await page.locator("[data-app-settings-trigger]").focus();
    await page.keyboard.press("Enter");
    const legend = page.locator("[data-app-settings-menu] legend");
    await expect(legend).toBeVisible();
    await expect(legend).toHaveCSS("font-size", "15px");
    const contrast = await legend.evaluate((element) => {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 1;
      const context = canvas.getContext("2d");
      const chain = [];
      for (let parent = element; parent; parent = parent.parentElement) chain.unshift(parent);
      context.fillStyle = "white";
      context.fillRect(0, 0, 1, 1);
      for (const parent of chain) {
        context.fillStyle = getComputedStyle(parent).backgroundColor;
        context.fillRect(0, 0, 1, 1);
      }
      const luminance = () => {
        const rgb = [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
        return rgb.reduce((sum, channel, index) => {
          const value = channel / 255;
          return (
            sum +
            (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4) *
              [0.2126, 0.7152, 0.0722][index]
          );
        }, 0);
      };
      const background = luminance();
      context.fillStyle = getComputedStyle(element).color;
      context.fillRect(0, 0, 1, 1);
      const foreground = luminance();
      return (Math.max(background, foreground) + 0.05) / (Math.min(background, foreground) + 0.05);
    });
    expect(contrast).toBeGreaterThanOrEqual(4.5);
    await page.keyboard.press("Escape");
  }
});

test.describe("touch appearance controls", () => {
  test.use({ hasTouch: true });
  test("retain their hit areas when text is reduced", async ({ page }) => {
    await groupPage(page);
    await page.evaluate(() => groupWorkProbe.setTextScale(70));
    await page.locator("[data-app-settings-trigger]").tap();
    await expect(page.locator("[data-app-settings-menu]")).toBeVisible();
    const controls = page.locator("[data-app-settings-menu] button");
    await expect
      .poll(() =>
        controls.evaluateAll((items) => items.every((e) => e.getBoundingClientRect().height >= 44)),
      )
      .toBe(true);
    const theme = page.locator('[data-appearance-select="theme"]');
    await theme.tap();
    await page.locator('[role="menuitemradio"][data-value="dark"]').tap();
    await expect(theme).toHaveAttribute("data-value", "dark");
    await expect(theme).toBeFocused();
  });
});
