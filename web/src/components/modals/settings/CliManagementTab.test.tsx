// @vitest-environment happy-dom
import { act } from "react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import * as api from "../../../services/api";
import { CliManagementTab } from "./CliManagementTab";
import { CliCronSummary, CliScheduleEditor } from "./CliScheduleEditor";
import zh from "../../../i18n/locales/zh/settings.json";
import en from "../../../i18n/locales/en/settings.json";
import ja from "../../../i18n/locales/ja/settings.json";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../../services/api", () => ({
  fetchCliManagement: vi.fn(),
  submitCliJob: vi.fn(),
  saveCliSchedules: vi.fn(),
  fetchCliJobLog: vi.fn(),
}));
// 表单单元测试使用原生 select 驱动值；真实 combobox 留给浏览器验收。
vi.mock("../../SelectCombobox", () => ({
  SelectCombobox: ({
    value,
    items,
    onChange,
    ariaLabel,
    disabled,
  }: {
    value: string;
    items: Array<{ value: string; label: string }>;
    onChange: (value: string) => void;
    ariaLabel: string;
    disabled?: boolean;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    >
      {items.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));

describe("CLI 管理原生设置", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let data: api.CliManagementStatus;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    data = {
      runtimes: [
        {
          name: "codex",
          display_name: "Codex CLI",
          command: "codex",
          external_available: true,
          external_path: "/usr/bin/codex",
          source: { kind: "mise", tool: "codex", node: false },
          installation: null,
          uninstall_available: false,
          uninstall_reason: "cli_not_managed",
        },
      ],
      state: { revision: 0, rules: [], installations: {}, jobs: {} },
    };
    vi.mocked(api.fetchCliManagement).mockImplementation(async () => ({ ok: true, result: data }));
    vi.mocked(api.fetchCliJobLog).mockResolvedValue({
      ok: true,
      result: { entries: [], next_offset: 0, has_more: false },
    });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });
  const button = (text: string) =>
    [...container.querySelectorAll("button")].find((item) => item.textContent === text)!;
  const mount = async () => {
    await act(async () => root.render(<CliManagementTab isDark={false} />));
  };
  const choose = async (label: string, value: string) => {
    await act(async () => {
      const select = container.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
      select.value = value;
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
  };

  it("简单计划使用易读摘要，复杂或不可等价的表达式保持原文及时区", async () => {
    for (const [cron, label] of [
      ["0 3 * * *", "cliManagement.scheduledaily"],
      ["05 03 * * 7", "cliManagement.scheduleweekly"],
      ["0 3 31 * *", "cliManagement.schedulemonthly"],
      ["0 3 * * 1,3", "0 3 * * 1,3"],
      ["0 25 * * *", "0 25 * * *"],
    ]) {
      await act(async () => root.render(<CliCronSummary cron={cron} timezone="Asia/Shanghai" />));
      expect(container.textContent).toBe(`${label} (Asia/Shanghai)`);
    }
  });

  it("后台存储故障显示错误，刷新恢复后清除错误且不触发软件操作", async () => {
    await mount();
    const message = "CLI 管理后台发生错误；请检查守护进程日志和存储状态";
    vi.mocked(api.fetchCliManagement).mockResolvedValueOnce({
      ok: false,
      error: { code: "cli_worker_unavailable", message },
    });
    await act(async () => button("cliManagement.refresh").click());
    expect(container.textContent).toContain(message);
    expect(container.textContent).toContain("Codex CLI");
    await act(async () => button("cliManagement.refresh").click());
    expect(container.textContent).not.toContain(message);
    expect(container.textContent).toContain("Codex CLI");
    expect(api.submitCliJob).not.toHaveBeenCalled();
    expect(api.saveCliSchedules).not.toHaveBeenCalled();
  });

  it("外部安装只展示默认来源，不能直接更新或卸载", async () => {
    await mount();
    expect(button("cliManagement.update").disabled).toBe(true);
    expect(container.textContent).toContain("cliManagement.externalInstallation");
    const uninstall = button("cliManagement.uninstall");
    expect(uninstall.disabled).toBe(true);
    await act(async () => uninstall.click());
    expect(button("cliManagement.installManaged")).toBeDefined();
    expect(api.submitCliJob).not.toHaveBeenCalled();
    expect(api.saveCliSchedules).not.toHaveBeenCalled();
  });

  it("只展示可管理 CLI，不展示 Web Model 和 Custom，也不修改原始运行时清单", async () => {
    for (const [name, display_name] of [
      ["web_model", "Web Model"],
      ["custom", "Custom"],
    ]) {
      data.runtimes.push({
        ...data.runtimes[0],
        name,
        display_name,
        source: { kind: "not_applicable", reason: "not_a_managed_cli" },
      });
    }
    await mount();
    expect([...container.querySelectorAll("h4")].map((item) => item.textContent)).toEqual([
      "Codex CLI",
    ]);
    expect(container.textContent).not.toContain("Web Model");
    expect(container.textContent).not.toContain("Custom");
    expect(data.runtimes.map((item) => item.name)).toEqual(["codex", "web_model", "custom"]);
    expect(api.submitCliJob).not.toHaveBeenCalled();
  });

  it("受管 CLI 更新按钮提交统一的 update 操作", async () => {
    data.runtimes[0].installation = {
      version: "1.0.0",
      executable: "/managed/codex",
      bin_paths: [],
      installed_at: "now",
    };
    vi.mocked(api.submitCliJob).mockResolvedValue({
      ok: true,
      result: {
        job: {
          id: "update-request",
          runtime: "codex",
          operation: "update",
          status: "queued",
          created_at: "now",
          started_at: null,
          finished_at: null,
          source_rule: null,
          error: null,
        },
      },
    });
    await mount();
    await act(async () => button("cliManagement.update").click());
    expect(api.submitCliJob).toHaveBeenCalledWith("codex", "update", expect.any(String));
  });

  it("受管卸载先确认，取消不提交；确认后进入任务列表且不能重复点击", async () => {
    data.runtimes[0].installation = {
      version: "1.0.0",
      executable: "/managed/codex",
      bin_paths: [],
      installed_at: "now",
    };
    data.runtimes[0].uninstall_available = true;
    const originalConfirm = window.confirm;
    const confirm = vi.fn(() => false);
    window.confirm = confirm;
    vi.mocked(api.submitCliJob).mockResolvedValue({
      ok: true,
      result: {
        job: {
          id: "remove",
          runtime: "codex",
          operation: "uninstall",
          status: "queued",
          created_at: "now",
          started_at: null,
          finished_at: null,
          source_rule: null,
          error: null,
        },
      },
    });
    try {
      await mount();
      expect(container.textContent).toContain("cliManagement.managedVersion");
      expect(container.textContent).not.toContain("cliManagement.externalInstallation");
      await act(async () => button("cliManagement.uninstall").click());
      expect(confirm).toHaveBeenCalledWith("cliManagement.uninstallConfirm");
      expect(api.submitCliJob).not.toHaveBeenCalled();
      confirm.mockReturnValue(true);
      data.state.jobs.remove = {
        id: "remove",
        runtime: "codex",
        operation: "uninstall",
        status: "queued",
        created_at: "now",
        started_at: null,
        finished_at: null,
        source_rule: null,
        error: null,
      };
      await act(async () => button("cliManagement.uninstall").click());
      expect(api.submitCliJob).toHaveBeenCalledWith("codex", "uninstall", expect.any(String));
      expect(button("cliManagement.uninstall").disabled).toBe(true);
      expect(container.textContent).toContain("cliManagement.status.queued");
    } finally {
      window.confirm = originalConfirm;
    }
  });

  it("日志分页后刷新当前尾部视图，读取失败可恢复且不触发安装", async () => {
    vi.useFakeTimers();
    try {
      data.state.jobs["log-test"] = {
        id: "log-test",
        runtime: "codex",
        operation: "install",
        status: "failed",
        created_at: new Date().toISOString(),
        started_at: null,
        finished_at: null,
        source_rule: null,
        error: "受控失败",
      };
      vi.mocked(api.fetchCliJobLog).mockImplementation(async (_job, offset) => ({
        ok: true,
        result: {
          entries: [
            {
              ts: "2026-09-10",
              stream: offset ? "stderr" : "stdout",
              text: offset ? "错误尾部 token=[REDACTED]" : "原始输出的脱敏尾部视图",
            },
          ],
          next_offset: offset ? 200 : 100,
          has_more: offset === 0,
        },
      }));
      await mount();
      await act(async () => button("cliManagement.viewLog").click());
      expect(container.querySelector("pre")?.textContent).toContain("脱敏尾部视图");
      expect(button("cliManagement.previousPage").disabled).toBe(true);
      await act(async () => button("cliManagement.nextPage").click());
      expect(api.fetchCliJobLog).toHaveBeenLastCalledWith("log-test", 100);
      expect(container.querySelector("pre")?.textContent).toContain("token=[REDACTED]");
      expect(button("cliManagement.nextPage").disabled).toBe(true);
      vi.mocked(api.fetchCliJobLog).mockResolvedValueOnce({
        ok: true,
        result: { entries: [], next_offset: 100, has_more: true },
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(button("cliManagement.nextPage").disabled).toBe(true);
      expect(api.fetchCliJobLog).toHaveBeenLastCalledWith("log-test", 100);
      vi.mocked(api.fetchCliJobLog).mockRejectedValueOnce(new Error("network"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(container.textContent).toContain("cliManagement.logFailed");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(api.fetchCliJobLog).toHaveBeenLastCalledWith("log-test", 100);
      expect(container.textContent).not.toContain("cliManagement.logFailed");
      await act(async () => button("cliManagement.previousPage").click());
      expect(api.fetchCliJobLog).toHaveBeenLastCalledWith("log-test", 0);
      expect(container.querySelector("pre")?.textContent).toContain("脱敏尾部视图");
      expect(api.submitCliJob).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it.each(["throw", "NETWORK_ERROR"])(
    "安装响应丢失（%s）时重用请求 ID，成功后显示任务且防止重复点击",
    async (failure) => {
      const job: api.CliJob = {
        id: "request",
        runtime: "codex",
        operation: "install",
        status: "running",
        created_at: new Date().toISOString(),
        started_at: null,
        finished_at: null,
        source_rule: null,
        error: null,
      };
      if (failure === "throw") {
        vi.mocked(api.submitCliJob).mockRejectedValueOnce(new Error("network"));
      } else {
        vi.mocked(api.submitCliJob).mockResolvedValueOnce({
          ok: false,
          error: { code: failure, message: "Failed to fetch" },
        });
      }
      vi.mocked(api.fetchCliJobLog).mockResolvedValueOnce({
        ok: false,
        error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
      });
      vi.mocked(api.submitCliJob).mockImplementationOnce(async () => {
        data = { ...data, state: { ...data.state, jobs: { request: job } } };
        return { ok: true, result: { job } };
      });
      await mount();
      await act(async () => button("cliManagement.installManaged").click());
      expect(container.textContent).toContain("cliManagement.requestUncertain");
      await act(async () => button("cliManagement.installManaged").click());
      const calls = vi.mocked(api.submitCliJob).mock.calls;
      expect(calls).toHaveLength(2);
      expect(calls[0]).toEqual(calls[1]);
      expect(calls[0][0]).toBe("codex");
      expect(button("cliManagement.installManaged").disabled).toBe(true);
      expect(container.textContent).toContain("cliManagement.status.running");
      expect(container.textContent).toContain("cliManagement.logFailed");
      expect(container.textContent).not.toContain("Failed to fetch");
    },
  );

  it("传输错误使用本地化指引，计划保存不确定时保留草稿", async () => {
    await mount();
    for (const code of ["NETWORK_ERROR", "EMPTY_RESPONSE", "PARSE_ERROR", "HTTP_ERROR"]) {
      vi.mocked(api.fetchCliManagement).mockResolvedValueOnce({
        ok: false,
        error: { code, message: "raw transport detail" },
      });
      await act(async () => button("cliManagement.refresh").click());
      expect(container.textContent).toContain("cliManagement.loadFailed");
      expect(container.textContent).not.toContain("raw transport detail");
    }
    vi.mocked(api.saveCliSchedules).mockResolvedValueOnce({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
    });
    await act(async () => button("cliManagement.addSchedule").click());
    const draftId = container.querySelector<HTMLInputElement>(
      'input[type="text"], input:not([type])',
    )!.value;
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(container.textContent).toContain("cliManagement.saveUncertain");
    expect(
      container.querySelector<HTMLInputElement>('input[type="text"], input:not([type])')!.value,
    ).toBe(draftId);
    await act(async () => button("common:cancel").click());
    expect(container.textContent).not.toContain("Failed to fetch");
    expect(api.submitCliJob).not.toHaveBeenCalled();
  });

  it("仅保留运行时、计划和任务日志，不显示专用错误报告配置", async () => {
    await mount();
    expect(container.textContent).toContain("cliManagement.schedules");
    expect(container.textContent).not.toContain("cliManagement.notification");
    expect(container.querySelector('input[type="password"]')).toBeNull();
    expect(api.submitCliJob).not.toHaveBeenCalled();
  });

  it("API 保存的复杂周期不伪装成每天，也不在保存时悄悄改写", async () => {
    const initial: api.CliSchedule = {
      id: "multi-day",
      enabled: false,
      trigger: { kind: "cron", cron: "0 3 * * 1,3", timezone: "Asia/Shanghai" },
    };
    const save = vi.fn(async () => {});
    await act(async () =>
      root.render(
        <CliScheduleEditor initial={initial} busy={false} onSave={save} onCancel={() => {}} />,
      ),
    );
    expect(container.textContent).toContain("cliManagement.customScheduleHint");
    expect(container.querySelector('select[aria-label="ruleEditor.pattern"]')).toBeNull();
    expect(container.querySelector('input[type="time"]')).toBeNull();
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(save).toHaveBeenCalledWith(initial);
  });

  it("新建计划默认 03:00 且不启用，保存冲突保留草稿", async () => {
    vi.mocked(api.saveCliSchedules).mockResolvedValue({
      ok: false,
      error: { code: "cli_schedule_revision_conflict", message: "conflict" },
    });
    await mount();
    await act(async () => button("cliManagement.addSchedule").click());
    expect(container.querySelector<HTMLInputElement>('input[type="time"]')?.value).toBe("03:00");
    expect(container.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(
      false,
    );
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const [revision, rules] = vi.mocked(api.saveCliSchedules).mock.calls[0];
    expect(revision).toBe(0);
    expect(rules[0]).toMatchObject({
      enabled: false,
      trigger: { kind: "cron", cron: "0 3 * * *" },
    });
    expect(rules[0]).not.toHaveProperty("next_run_at");
    expect(container.textContent).toContain("cliManagement.scheduleConflict");
    expect(container.querySelector("form")).not.toBeNull();
  });

  it("周期编辑保留原时区并沿用星期和月末表达式", async () => {
    const onSave = vi.fn(async () => undefined);
    await act(async () =>
      root.render(
        <CliScheduleEditor
          initial={{
            id: "weekly",
            enabled: true,
            trigger: { kind: "cron", cron: "0 3 * * 1", timezone: "Asia/Tokyo" },
          }}
          busy={false}
          onSave={onSave}
          onCancel={() => undefined}
        />,
      ),
    );
    await choose("ruleEditor.weekday", "0");
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(onSave).toHaveBeenLastCalledWith({
      id: "weekly",
      enabled: true,
      trigger: { kind: "cron", cron: "0 3 * * 0", timezone: "Asia/Tokyo" },
    });
    await choose("ruleEditor.pattern", "monthly");
    expect(container.querySelector<HTMLInputElement>('input[type="number"]')?.max).toBe("31");
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(onSave).toHaveBeenLastCalledWith({
      id: "weekly",
      enabled: true,
      trigger: { kind: "cron", cron: "0 3 1 * *", timezone: "Asia/Tokyo" },
    });
  });

  it("一次性倒计时以保存时刻计算，不提前调用安装接口", async () => {
    const onSave = vi.fn(async () => undefined);
    await act(async () =>
      root.render(
        <CliScheduleEditor
          initial={{
            id: "once",
            enabled: true,
            trigger: { kind: "interval", every_seconds: 3600 },
          }}
          busy={false}
          onSave={onSave}
          onCancel={() => undefined}
        />,
      ),
    );
    await choose("ruleEditor.scheduleType", "at");
    const before = Date.now();
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const saved = onSave.mock.calls[0] as unknown as [api.CliSchedule];
    expect(saved[0].trigger.kind).toBe("at");
    if (saved[0].trigger.kind === "at") {
      expect(Date.parse(saved[0].trigger.at)).toBeGreaterThanOrEqual(before + 30 * 60_000);
      expect(Date.parse(saved[0].trigger.at)).toBeLessThanOrEqual(Date.now() + 30 * 60_000);
    }
    expect(api.submitCliJob).not.toHaveBeenCalled();
  });

  it.each(["seconds", "_seconds", "-seconds"])(
    "计划 %s 的 90 秒间隔可保存，查看全部会请求归档",
    async (id) => {
      const onSave = vi.fn(async () => undefined);
      await act(async () =>
        root.render(
          <CliScheduleEditor
            initial={{ id, enabled: true, trigger: { kind: "interval", every_seconds: 90 } }}
            busy={false}
            onSave={onSave}
            onCancel={() => undefined}
          />,
        ),
      );
      const input = container.querySelector<HTMLInputElement>('input[type="number"]')!;
      expect(input.value).toBe("1.5");
      expect(input.checkValidity()).toBe(true);
      await act(async () => container.querySelector("form")!.requestSubmit());
      expect(onSave).toHaveBeenCalledWith({
        id,
        enabled: true,
        trigger: { kind: "interval", every_seconds: 90 },
      });
      for (let index = 0; index < 21; index++) {
        const id = `history-${index}`;
        data.state.jobs[id] = {
          id,
          runtime: "codex",
          operation: "install",
          status: "failed",
          created_at: "2026-09-10T00:00:00Z",
          started_at: null,
          finished_at: null,
          source_rule: null,
          error: "fixture",
        };
      }
      await mount();
      expect(api.fetchCliManagement).toHaveBeenLastCalledWith(false);
      await act(async () => button("cliManagement.allJobs").click());
      expect(api.fetchCliManagement).toHaveBeenLastCalledWith(true);
    },
  );

  it("操作记录的状态、来源、时间与日志编号逐项对应，切换记录不串日志", async () => {
    const statuses: api.CliJob["status"][] = [
      "queued",
      "running",
      "succeeded",
      "failed",
      "interrupted",
    ];
    statuses.forEach((status, index) => {
      const id = `record-${index}`;
      data.state.jobs[id] = {
        id,
        runtime: index % 2 ? "claude" : "codex",
        operation: index % 2 ? "update" : "install",
        status,
        created_at: `2026-09-10T00:0${index}:00Z`,
        started_at: index ? `2026-09-10T00:0${index}:01Z` : null,
        finished_at: index > 1 ? `2026-09-10T00:0${index}:02Z` : null,
        source_rule: index % 2 ? "nightly" : null,
        error: status === "failed" ? "controlled failure" : null,
      };
    });
    vi.mocked(api.fetchCliJobLog).mockImplementation(async (id) => ({
      ok: true,
      result: {
        entries: [{ ts: "2026-09-10", stream: "stage", text: `log-for-${id}` }],
        next_offset: 1,
        has_more: false,
      },
    }));
    await mount();
    for (const job of Object.values(data.state.jobs)) {
      const row = container.querySelector(`[data-job-id="${job.id}"]`)!;
      expect(row.textContent).toContain(job.runtime);
      expect(row.textContent).toContain(`cliManagement.${job.operation}`);
      expect(row.textContent).toContain(`cliManagement.status.${job.status}`);
      expect(row.textContent).toContain(
        job.source_rule ? "cliManagement.scheduledOperation" : "cliManagement.manualOperation",
      );
      for (const key of ["created_at", "started_at", "finished_at"] as const) {
        if (job[key]) expect(row.textContent).toContain(new Date(job[key]!).toLocaleString());
      }
      if (job.error) expect(row.textContent).toContain(job.error);
      await act(async () => row.querySelector<HTMLButtonElement>("button")!.click());
      expect(api.fetchCliJobLog).toHaveBeenLastCalledWith(job.id, 0);
      expect(container.querySelector("pre")!.textContent).toBe(
        `2026-09-10 [stage] log-for-${job.id}`,
      );
    }
    expect(api.submitCliJob).not.toHaveBeenCalled();
  });

  it("三种语言的管理文案和状态键保持一致", () => {
    expect(JSON.stringify(zh.cliManagement)).not.toMatch(/纳管/);
    expect(zh.cliManagement.jobs).toBe("CLI 操作记录");
    expect(zh.cliManagement.schedules).toBe("CLI 自动更新计划");
    expect(zh.cliManagement.update).toBe("更新");
    expect(en.cliManagement.update).toBe("Update");
    for (const locale of [zh, en, ja]) {
      expect(locale.cliManagement).not.toHaveProperty("upgrade");
      expect(JSON.stringify(locale.cliManagement)).not.toMatch(/upgrade|升级/i);
    }
    for (const locale of [zh, ja]) {
      expect(Object.keys(locale.cliManagement).sort()).toEqual(
        Object.keys(en.cliManagement).sort(),
      );
      expect(Object.keys(locale.cliManagement.status).sort()).toEqual(
        Object.keys(en.cliManagement.status).sort(),
      );
      expect(locale.tabs.cliManagement).toBeTruthy();
    }
  });
  it("当前 CLI 文档没有旧术语或卸载占位合同", () => {
    for (const path of ["docs/guide/cli-management.md", "docs/specs/cli-management.md"]) {
      const content = readFileSync(resolve(process.cwd(), "..", path), "utf8");
      expect(content).not.toMatch(/升级|纳管|cli_uninstall_not_implemented|HTTP 501/);
    }
  });
});
