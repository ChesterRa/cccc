#!/usr/bin/env python3
"""Linux/Windows 真实 CCCC Web 验收；仅使用新建、无凭据的隔离目录。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import urllib.request
import urllib.error

parser = argparse.ArgumentParser()
parser.add_argument("--binary", required=True, type=Path)
parser.add_argument("--root", required=True, type=Path)
parser.add_argument("--prepare-only", action="store_true")
args = parser.parse_args()
root = args.root.resolve()
binary = args.binary.resolve(strict=True)
root.mkdir(parents=True, exist_ok=False)
for name in ("bin", "releases", "user", "home", "workspace", "evidence"):
    (root / name).mkdir()
# 原生 attach 以 Git 根目录确定 Scope，不能让夹具向上归并到源码仓库。
subprocess.run(["git", "-c", "init.defaultBranch=main", "init", "--quiet", str(root / "workspace")], check=True)
(root / "fixture-marker").write_text("controlled-cli-only", encoding="utf-8")
(root / "target-version").write_text("1.0.0", encoding="utf-8")
fixture = Path(__file__).with_name("cli-management-fixture.py").resolve()
if os.name == "nt":
    wrapper = f'@echo off\r\n"{sys.executable}" "{fixture}" "{root}" %*\r\n'
else:
    import shlex
    wrapper = f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(fixture))} {shlex.quote(str(root))} "$@"\n'
mise = root / "bin" / ("mise.cmd" if os.name == "nt" else "mise")
mise.write_text(wrapper, encoding="utf-8", newline="")
mise.chmod(0o700)
for version in ("0.9.0", "1.0.0", "2.0.0", "3.0.0"):
    exit_code = 7 if version == "3.0.0" else 0
    if os.name == "nt":
        code = f'@echo off\r\nif "%~1"=="--version" (\r\n echo fixture {version}\r\n exit /b {exit_code}\r\n)\r\necho {version}>"%CCCC_HOME%\\%CCCC_ACTOR_ID%.marker"\r\ncmd.exe /Q\r\n'
    else:
        code = f'#!/bin/sh\nif [ "${{1:-}}" = --version ]; then echo "fixture {version}"; exit {exit_code}; fi\nprintf "{version}\\n" > "$CCCC_HOME/$CCCC_ACTOR_ID.marker"\nwhile IFS= read -r line; do :; done\n'
    release = root / "releases" / version
    release.write_text(code, encoding="utf-8", newline="")
    release.chmod(0o700)
for name in ("grok", "codex", "agy"):
    target = root / "bin" / (name + (".cmd" if os.name == "nt" else ""))
    shutil.copyfile(root / "releases/0.9.0", target)
    target.chmod(0o700)
sentinel = root / "user/login-session.fixture"
sentinel.write_bytes(b"synthetic-login-and-session")
protected = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
             for path in [sentinel, *[path for path in (root / "bin").iterdir() if path != mise]]}
env = os.environ.copy()
for key in list(env):
    if any(word in key.upper() for word in ("TOKEN", "SECRET", "PASSWORD", "API_KEY")):
        env.pop(key)
env.update({"CCCC_HOME": str(root / "home"), "HOME": str(root / "user"),
            "USERPROFILE": str(root / "user"), "PATH": str(root / "bin") + os.pathsep + env.get("PATH", "")})
(root / "environment.json").write_text(json.dumps({key: env[key] for key in ("CCCC_HOME", "HOME", "USERPROFILE", "PATH")}), encoding="utf-8")
if args.prepare_only:
    print(root)
    raise SystemExit(0)

base = "http://127.0.0.1:8863/"
session = "cli-management-" + root.name
evidence = root / "evidence"

def browser(*arguments):
    result = subprocess.run(["agent-browser", "--session", session, *arguments],
                            capture_output=True, text=True, encoding="utf-8", timeout=40)
    with (evidence / "browser.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps({"args": arguments, "code": result.returncode,
                              "out": result.stdout, "err": result.stderr}, ensure_ascii=False) + "\n")
    if result.returncode:
        raise RuntimeError(result.stderr or result.stdout)
    return result.stdout

def api(path):
    request = urllib.request.Request(base + "api/v1/" + path, headers={"Origin": base.rstrip("/")})
    with urllib.request.urlopen(request, timeout=10) as response:
        data = json.load(response)
    assert data["ok"], data
    return data["result"]

def click(name):
    browser("wait", "--fn", "[...document.querySelectorAll('button')].some(e=>e.getClientRects().length && (e.getAttribute('aria-label')===" + json.dumps(name) + " || e.textContent.trim()===" + json.dumps(name) + "))")
    browser("find", "role", "button", "click", "--name", name, "--exact")

def idle(expected):
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        state = api("cli-management")["state"]
        if len(state["jobs"]) == expected and all(job["status"] not in ("running", "queued") for job in state["jobs"].values()):
            click("刷新")
            return state
        time.sleep(0.2)
    raise AssertionError("操作未结束或数量不符")

def evaluate(expression):
    value = json.loads(browser("eval", "JSON.stringify(" + expression + ")"))
    return json.loads(value) if isinstance(value, str) else value

def choose(label, name):
    selector = '[role="combobox"][aria-label=' + json.dumps(label, ensure_ascii=False) + ']'
    browser("wait", selector)
    browser("click", selector)
    browser("find", "role", "option", "click", "--name", name, "--exact")

def check_records():
    state = api("cli-management?history=true")["state"]
    labels = {"queued": "排队中", "running": "执行中", "succeeded": "成功", "failed": "失败", "interrupted": "已中断"}
    expected = {key: labels[job["status"]] for key, job in state["jobs"].items()}
    browser("wait", "--fn", "Array.from(document.querySelectorAll('[data-job-id]')).every(e=>e.innerText.includes((" + json.dumps(expected) + ")[e.dataset.jobId]))")
    rows = evaluate("Array.from(document.querySelectorAll('[data-job-id]'),e=>({id:e.dataset.jobId,text:e.innerText}))")
    assert len(rows) == min(20, len(state["jobs"])) or len(rows) == len(state["jobs"])
    for row in rows:
        job = state["jobs"][row["id"]]
        assert job["runtime"] in row["text"]
        assert {"install": "安装", "update": "更新", "uninstall": "卸载受管版本"}[job["operation"]] in row["text"]
        assert {"queued": "排队中", "running": "执行中", "succeeded": "成功", "failed": "失败", "interrupted": "已中断"}[job["status"]] in row["text"]
        assert (job["source_rule"] or "手动操作") in row["text"]
        for key in ("created_at", "started_at", "finished_at"):
            if job[key]:
                assert evaluate("new Date(" + json.dumps(job[key]) + ").toLocaleString()") in row["text"]
    intervals = sorted((job["started_at"], job["finished_at"]) for job in state["jobs"].values() if job["started_at"] and job["finished_at"])
    assert all(before[1] <= after[0] for before, after in zip(intervals, intervals[1:]))
    (evidence / "records.json").write_text(json.dumps({"state": state, "rows": rows}, ensure_ascii=False, indent=2), encoding="utf-8")

def check_log(job_id):
    selector = '[data-job-id=' + json.dumps(job_id) + '] button'
    browser("scrollintoview", selector)
    browser("click", selector)
    offset = 0
    pages = []
    while True:
        page = api(f"cli-management/jobs/{job_id}/log?offset={offset}")
        expected = "\n".join(f'{entry["ts"]} [{entry["stream"]}] {entry["text"].rstrip()}' for entry in page["entries"])
        browser("wait", "--fn", "document.querySelector('pre[aria-label=\"操作日志\"]')?.textContent === " + json.dumps(expected))
        assert "web-fixture-secret" not in expected
        pages.append(page)
        if not page["has_more"]:
            break
        assert page["next_offset"] > offset
        click("下一页")
        offset = page["next_offset"]
    if len(pages) > 1:
        click("上一页")
    (evidence / f"log-{job_id}.json").write_text(json.dumps(pages, ensure_ascii=False, indent=2), encoding="utf-8")

def open_management(language="zh"):
    locale = json.loads((Path(__file__).resolve().parents[2] / "src/i18n/locales" / language / "settings.json").read_text(encoding="utf-8"))
    browser("wait", "[data-app-settings-trigger]")
    browser("click", "[data-app-settings-trigger]")
    click(locale["title"])
    browser("wait", "--text", locale["navigation"]["globalScopeTitle"])
    scope = locale["navigation"]["global"] + " " + locale["navigation"]["globalScopeTitle"]
    browser("find", "role", "button", "click", "--name", scope, "--exact")
    click(locale["cliManagement"]["title"])
    browser("wait", "--text", locale["cliManagement"]["schedules"])

def actor_status(actor_id):
    return next(actor for actor in api(f"groups/{group_id}/actors")["actors"] if actor["id"] == actor_id)

def wait_actor(actor_id, running, version=None):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        actor = actor_status(actor_id)
        marker = root / "home" / (actor_id + ".marker")
        if actor["running"] == running and (version is None or (marker.exists() and marker.read_text().strip() == version)):
            with (evidence / "actors.jsonl").open("a", encoding="utf-8") as log:
                log.write(json.dumps({"actor": actor, "expected_version": version}, ensure_ascii=False) + "\n")
            return actor
        time.sleep(0.2)
    raise AssertionError(f"Actor {actor_id} 未达到 running={running}, version={version}: {actor}")

def actor_action(actor_id, action):
    browser("reload")
    click(f"打开 {actor_id} 的终端")
    selector = '[aria-label=' + json.dumps(action, ensure_ascii=False) + ']'
    browser("wait", selector)
    browser("find", "first", selector, "click")

def create_actor(actor_id, explicit=False):
    browser("reload")
    click("添加智能体")
    modal = '[aria-labelledby="actor-config-create-title"]'
    browser("wait", modal)
    choose("运行时", "Antigravity CLI")
    browser("fill", modal + " input[placeholder]", actor_id)
    if explicit:
        browser("scrollintoview", modal + " input[type=checkbox]")
        browser("uncheck", modal + " input[type=checkbox]")
        browser("fill", modal + " input.font-mono", '"' + str(root / "bin" / ("agy.cmd" if os.name == "nt" else "agy")) + '"')
    browser("click", modal + ' button.w-full.font-semibold')
    browser("wait", "--fn", "!document.querySelector(" + json.dumps(modal) + ")")
    actor_action(actor_id, "启动智能体")

output = (evidence / "server.log").open("w", encoding="utf-8")
server = subprocess.Popen([str(binary), "--host", "127.0.0.1", "--port", "8863"],
                          cwd=root / "workspace", env=env, stdout=output, stderr=subprocess.STDOUT)
try:
    deadline = time.monotonic() + 30
    while True:
        try:
            api("cli-management")
            break
        except (OSError, KeyError):
            if server.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("隔离 CCCC 服务启动失败")
            time.sleep(0.2)
    attached = subprocess.run([str(binary), "attach", str(root / "workspace")], env=env,
                              cwd=root / "workspace", capture_output=True, text=True, encoding="utf-8", timeout=20, check=True)
    (evidence / "group.json").write_text(attached.stdout, encoding="utf-8")
    group_id = json.loads(attached.stdout)["group_id"]
    browser("open", base)
    browser("eval", "localStorage.setItem('cccc-language','zh')")
    browser("reload")
    open_management()
    status = api("cli-management")
    for runtime in status["runtimes"]:
        if runtime["source"]["kind"] == "not_applicable":
            browser("wait", "--fn", "![...document.querySelectorAll('h4')].some(e=>e.textContent===" + json.dumps(runtime["display_name"]) + ")")
    browser("screenshot", str(evidence / "platform.png"))
    click("安装受管版本 Grok")
    installed = idle(1)
    assert installed["installations"]["grok"]["version"] == "1.0.0"
    (root / "target-version").write_text("2.0.0", encoding="utf-8")
    click("更新 Grok")
    updated = idle(2)
    assert updated["installations"]["grok"]["version"] == "2.0.0"
    (root / "target-version").write_text("3.0.0", encoding="utf-8")
    click("更新 Grok")
    failed = idle(3)
    assert failed["installations"] == updated["installations"]
    assert sorted(failed["jobs"].values(), key=lambda job: job["created_at"])[-1]["status"] == "failed"
    browser("screenshot", str(evidence / "failure.png"))
    check_records()
    for job in sorted(failed["jobs"].values(), key=lambda item: item["created_at"])[::2]:
        check_log(job["id"])
    # 原生确认框必须真实点击，不能替换 window.confirm。
    click("卸载受管版本 Grok")
    browser("dialog", "dismiss")
    assert len(api("cli-management")["state"]["jobs"]) == 3
    click("卸载受管版本 Grok")
    browser("dialog", "accept")
    removed = idle(4)
    assert "grok" not in removed["installations"]
    for path, digest in protected.items():
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest, path
    browser("screenshot", str(evidence / "uninstall.png"))
    for name, kind in (("daily-check", "每天"), ("weekly-check", "每周"), ("monthly-check", "每月"), ("interval-check", "间隔调度")):
        click("添加计划")
        browser("find", "label", "规则名称（ID）", "fill", name)
        browser("wait", "--fn", "document.querySelector('input[type=time]').value === '03:00' && !document.querySelector('form input[type=checkbox]').checked")
        if kind == "间隔调度":
            choose("调度类型", kind)
            browser("find", "label", "重复间隔（分钟）", "fill", "15")
        elif kind != "每天":
            choose("模式", kind)
            if kind == "每周":
                choose("星期", "周日")
            else:
                browser("find", "label", "日期", "fill", "31")
        click("保存")
        browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
        rule = next(rule for rule in api("cli-management")["state"]["rules"] if rule["id"] == name)
        assert not rule["enabled"]
        if kind == "间隔调度":
            assert rule["trigger"]["every_seconds"] == 900
        else:
            assert rule["trigger"]["cron"] == {"每天": "0 3 * * *", "每周": "0 3 * * 0", "每月": "0 3 31 * *"}[kind]
    browser("reload")
    open_management()
    assert len(api("cli-management")["state"]["rules"]) == 4
    for name in ("daily-check", "weekly-check", "monthly-check", "interval-check"):
        click("移除计划 " + name)
        browser("wait", "--fn", "![...document.querySelectorAll('button')].some(e=>e.getAttribute('aria-label')===" + json.dumps("移除计划 " + name) + ")")
    assert not api("cli-management")["state"]["rules"]
    # 验证依赖缺失是失败而不是假成功；只改动本次测试自己的替身入口。
    disabled_mise = mise.with_name(mise.name + ".disabled")
    mise.rename(disabled_mise)
    try:
        click("安装受管版本 Grok")
        missing = idle(5)
        assert not missing["installations"]
        assert "mise" in sorted(missing["jobs"].values(), key=lambda job: job["created_at"])[-1]["error"]
    finally:
        disabled_mise.rename(mise)
    (root / "target-version").write_text("2.0.0", encoding="utf-8")
    click("安装受管版本 Grok")
    healthy = idle(6)
    selected = Path(healthy["installations"]["grok"]["executable"])
    assert selected.resolve().is_relative_to(root / "home/cli-management/versions")
    damaged = selected.with_name(selected.name + ".damaged")
    selected.rename(damaged)
    click("刷新")
    browser("wait", "--text", "受管安装不可用")
    click("更新 Grok")
    repaired = idle(7)
    assert repaired["installations"]["grok"]["version"] == "2.0.0"
    assert repaired["installations"]["grok"]["executable"] != str(selected)
    assert Path(repaired["installations"]["grok"]["executable"]).is_file()
    check_records()
    # Antigravity 沿用原生 PTY 路径；不伪造 Grok/Codex 的托管协议握手。
    (root / "target-version").write_text("1.0.0", encoding="utf-8")
    click("安装受管版本 Antigravity")
    idle(8)
    create_actor("managed")
    old = wait_actor("managed", True, "1.0.0")
    create_actor("explicit", explicit=True)
    wait_actor("explicit", True, "0.9.0")
    browser("reload")
    open_management()
    (root / "target-version").write_text("2.0.0", encoding="utf-8")
    click("更新 Antigravity")
    idle(9)
    assert wait_actor("managed", True, "1.0.0")["pid"] == old["pid"]
    actor_action("managed", "重启智能体")
    wait_actor("managed", True, "2.0.0")
    browser("reload")
    open_management()
    click("卸载受管版本 Antigravity")
    browser("dialog", "accept")
    busy = idle(10)
    assert "antigravity" in busy["installations"]
    assert sorted(busy["jobs"].values(), key=lambda job: job["created_at"])[-1]["status"] == "failed"
    actor_action("managed", "停止智能体")
    wait_actor("managed", False)
    # 当前使用锁按 Runtime 保护；显式外部命令的同类 Actor 也须先停止。
    actor_action("explicit", "停止智能体")
    wait_actor("explicit", False)
    browser("reload")
    open_management()
    click("卸载受管版本 Antigravity")
    browser("dialog", "accept")
    removed = idle(11)
    assert "antigravity" not in removed["installations"]
    actor_action("managed", "启动智能体")
    wait_actor("managed", True, "0.9.0")
    actor_action("explicit", "启动智能体")
    wait_actor("explicit", True, "0.9.0")
    for path, digest in protected.items():
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest, path
    browser("reload")
    open_management()
    check_records()
    click("安装受管版本 Codex CLI")
    idle(12)
    click("添加计划")
    browser("find", "label", "规则名称（ID）", "fill", "background-once")
    choose("调度类型", "一次性调度")
    browser("find", "label", "多少分钟后执行", "fill", "1")
    browser("check", "form input[type=checkbox]")
    click("保存")
    browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
    scheduled = api("cli-management")["state"]
    browser("open", "about:blank")
    deadline = time.monotonic() + 100
    while time.monotonic() < deadline:
        completed = api("cli-management")["state"]
        jobs = [job for job in completed["jobs"].values() if job["source_rule"] == "background-once"]
        if len(jobs) == 2 and all(job["status"] == "succeeded" for job in jobs):
            break
        time.sleep(0.5)
    else:
        raise AssertionError("关闭页面后一次性计划未成功更新两个受管 CLI")
    assert {job["runtime"] for job in jobs} == {"grok", "codex"}
    intervals = sorted((job["started_at"], job["finished_at"]) for job in jobs)
    assert intervals[0][1] <= intervals[1][0]
    assert next(rule for rule in completed["rules"] if rule["id"] == "background-once")["next_run_at"] is None
    (evidence / "background-schedule.json").write_text(json.dumps({"before": scheduled, "after": completed}, ensure_ascii=False, indent=2), encoding="utf-8")
    browser("open", base)
    open_management()
    check_records()
    click("移除计划 background-once")
    # 无需供应商访问的同版本检查，用真实操作累计记录并验证最近 20/全部切换。
    for count in range(15, 22):
        click("更新 Grok")
        idle(count)
    browser("wait", "--fn", "document.querySelectorAll('[data-job-id]').length === 20")
    click("显示全部记录")
    browser("wait", "--fn", "document.querySelectorAll('[data-job-id]').length === 21")
    check_records()
    # 仅损坏本次隔离状态，验证管理页显式报错而原生外部 Runtime 仍可见。
    state_file = root / "home/cli-management/state.json"
    saved = state_file.read_bytes()
    try:
        state_file.write_text("broken-fixture", encoding="utf-8")
        click("刷新")
        browser("wait", "[role=alert]")
        native = api("runtimes")["runtimes"]
        assert all(next(item for item in native if item["name"] == name)["available"] for name in ("grok", "codex", "antigravity"))
    finally:
        state_file.write_bytes(saved)
    click("刷新")
    browser("wait", "--fn", "!document.querySelector('[role=alert]')")
    assert len(api("cli-management")["state"]["jobs"]) == 21
    check_records()
    # 浏览器真实断网；不得将后端失败替换成伪造的成功响应。
    browser("set", "offline", "on")
    try:
        click("刷新")
        browser("wait", "[role=alert]")
        browser("screenshot", str(evidence / "offline.png"))
    finally:
        browser("set", "offline", "off")
    click("刷新")
    browser("wait", "--fn", "!document.querySelector('[role=alert]')")
    # 无效名称不得保存；有效计划的编辑、启停与删除沿用真实表单。
    click("添加计划")
    browser("find", "label", "规则名称（ID）", "fill", "invalid.name")
    click("保存")
    browser("wait", "--text", "名称只能含英文字母")
    assert not api("cli-management")["state"]["rules"]
    browser("find", "label", "规则名称（ID）", "fill", "edit-check")
    click("保存")
    browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
    click("编辑")
    browser("check", "form input[type=checkbox]")
    click("保存")
    browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
    assert api("cli-management")["state"]["rules"][0]["enabled"]
    click("编辑")
    browser("uncheck", "form input[type=checkbox]")
    click("保存")
    browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
    assert not api("cli-management")["state"]["rules"][0]["enabled"]
    click("移除计划 edit-check")
    browser("wait", "--fn", "!document.querySelector('[aria-label=\"移除计划 edit-check\"]')")
    # 注入仅属于本次测试的旧复杂计划，验证页面不把它重写成默认周期。
    state_file = root / "home/cli-management/state.json"
    preserved = json.loads(state_file.read_text())
    preserved["rules"] = [{"id": "complex-check", "enabled": False,
                           "trigger": {"kind": "cron", "cron": "7 4 * * 1,3,5", "timezone": "UTC"},
                           "next_run_at": None}]
    state_file.write_text(json.dumps(preserved), encoding="utf-8")
    click("刷新")
    browser("wait", "--text", "complex-check")
    click("编辑")
    browser("wait", "--text", "此周期表达式不能用预设控件表示")
    click("保存")
    browser("wait", "--fn", "!document.querySelector('form input[maxlength=\"64\"]')")
    assert api("cli-management")["state"]["rules"][0]["trigger"]["cron"] == "7 4 * * 1,3,5"
    click("编辑")
    changed = json.loads(state_file.read_text())
    changed["revision"] += 1
    state_file.write_text(json.dumps(changed), encoding="utf-8")
    click("保存")
    browser("wait", "--text", "计划已被其他操作修改")
    click("取消")
    click("刷新")
    click("移除计划 complex-check")
    browser("wait", "--fn", "!document.querySelector('[aria-label=\"移除计划 complex-check\"]')")
    click("添加计划")
    browser("find", "label", "规则名称（ID）", "fill", "past-check")
    choose("调度类型", "一次性调度")
    choose("一次性模式", "精确时间")
    # datetime-local 是浏览器分段控件，普通文本 fill 会清空而非提交年份。
    browser("find", "role", "spinbutton", "click", "--name", "Year Year", "--exact")
    browser("press", "ArrowDown")
    browser("press", "Tab")
    assert evaluate("Date.parse(document.querySelector('input[type=datetime-local]').value) < Date.now()")
    browser("check", "form input[type=checkbox]")
    click("保存")
    browser("wait", "--text", "请选择将来的执行时间")
    assert not api("cli-management")["state"]["rules"]
    click("取消")
    # 日志损坏、恢复及未完成的 JSON 行均经真实日志 API 与页面核对。
    log_job = sorted(api("cli-management")["state"]["jobs"].values(), key=lambda job: job["created_at"])[-1]["id"]
    log_path = root / "home/cli-management/logs" / (log_job + ".jsonl")
    log_saved = log_path.read_bytes()
    try:
        log_path.write_bytes(b'{"incomplete":')
        browser("click", '[data-job-id=' + json.dumps(log_job) + '] button')
        browser("wait", "--text", "暂无日志，正在等待操作输出")
        assert evaluate("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='下一页').disabled")
        page = api(f"cli-management/jobs/{log_job}/log?offset=0")
        assert page["next_offset"] == 0
        log_path.write_bytes(b'invalid-json\n')
        browser("wait", "[role=alert]")
    finally:
        log_path.write_bytes(log_saved)
    browser("wait", "--fn", "!document.querySelector('[role=alert]')")
    check_log(log_job)
    # CSRF 负例只发送应被拒绝的请求，不能创建后台操作。
    request = urllib.request.Request(base + "api/v1/cli-management/jobs", method="POST",
                                     headers={"Origin": "https://invalid.example", "Content-Type": "application/json"},
                                     data=b'{"runtime":"grok","operation":"update","request_id":"csrf-fixture"}')
    try:
        urllib.request.urlopen(request, timeout=10)
        raise AssertionError("跨站写请求未被拒绝")
    except urllib.error.HTTPError as rejected:
        assert rejected.code in (401, 403)
    assert len(api("cli-management")["state"]["jobs"]) == 21
    # 真正运行/排队的操作经原生 Daemon 关闭而中断，不在状态文件伪造终态。
    (root / "target-version").write_text("1.0.0", encoding="utf-8")
    hold = root / "hold-install"
    hold.touch()
    browser("set", "offline", "on")
    try:
        click("更新 Grok")
        browser("wait", "--text", "请求结果尚未确认")
        assert len(api("cli-management")["state"]["jobs"]) == 21
    finally:
        browser("set", "offline", "off")
    click("更新 Grok")
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        active = api("cli-management")["state"]
        if any(job["status"] == "running" for job in active["jobs"].values()):
            break
        time.sleep(0.2)
    else:
        raise AssertionError("未观察到真实执行中操作")
    assert len(active["jobs"]) == 22
    browser("wait", "--fn", "document.querySelector('[aria-label=\"更新 Grok\"]').disabled")
    click("更新 Codex CLI")
    browser("wait", "--text", "排队中")
    check_records()
    subprocess.run([str(binary), "daemon", "stop"], env=env, capture_output=True, timeout=20, check=True)
    server.wait(timeout=15)
    hold.unlink()
    server = subprocess.Popen([str(binary), "--host", "127.0.0.1", "--port", "8863"],
                              cwd=root / "workspace", env=env, stdout=output, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            api("cli-management")
            break
        except OSError:
            time.sleep(0.2)
    browser("reload")
    open_management()
    restarted = idle(23)
    assert any(job["status"] == "interrupted" for job in restarted["jobs"].values())
    assert sorted(restarted["jobs"].values(), key=lambda job: job["created_at"])[-1]["status"] == "succeeded"
    click("更新 Grok")
    idle(24)
    # 按原生归档格式准备既有历史；自动归档阈值另由核心测试覆盖。
    archived_state = json.loads(state_file.read_text())
    oldest = min(archived_state["jobs"].values(), key=lambda job: job["created_at"])
    archive = root / "home/cli-management/history"
    archive.mkdir(exist_ok=True)
    (archive / (oldest["id"].encode().hex() + ".json")).write_text(json.dumps(oldest), encoding="utf-8")
    archived_state["jobs"].pop(oldest["id"])
    state_file.write_text(json.dumps(archived_state), encoding="utf-8")
    click("刷新")
    click("显示全部记录")
    browser("wait", "--fn", "document.querySelectorAll('[data-job-id]').length === 24")
    check_records()
    check_log(oldest["id"])
    # 三语/主题/窄屏使用同一真实后端；保存 DOM 与截图，检查键盘焦点可达。
    for language in ("zh", "en", "ja"):
        for theme in ("light", "dark"):
            browser("set", "viewport", "1280", "900")
            browser("eval", "localStorage.setItem('cccc-language'," + json.dumps(language) + ");localStorage.setItem('cccc-theme'," + json.dumps(theme) + ")")
            browser("set", "media", theme)
            browser("reload")
            browser("wait", "[data-app-settings-trigger]")
            open_management(language)
            browser("set", "viewport", "430", "900")
            browser("press", "Tab")
            assert evaluate("document.activeElement !== document.body")
            browser("screenshot", str(evidence / f"layout-{language}-{theme}.png"))
    browser("set", "viewport", "1280", "900")
    browser("eval", "localStorage.setItem('cccc-language','zh')")
    browser("reload")
    open_management()
    # 合成受限身份仅用于隔离实例；不接触测试服真实账号或令牌。
    access_file = root / "home/access_tokens.yaml"
    access_saved = access_file.read_bytes() if access_file.exists() else None
    restricted = "local-gui-restricted-fixture"
    # 原生实现无管理员时处于 bootstrap；模拟已配置实例须同时有管理员记录。
    access_file.write_text(json.dumps({"tokens": {
        restricted: {"user_id": "gui-fixture", "allowed_groups": [group_id], "is_admin": False,
                     "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"},
        "local-gui-admin-fixture": {"user_id": "gui-admin-fixture", "allowed_groups": [], "is_admin": True,
                                    "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"}}}), encoding="utf-8")
    try:
        request = urllib.request.Request(base + "api/v1/cli-management", headers={"Authorization": "Bearer " + restricted})
        try:
            urllib.request.urlopen(request, timeout=10)
            raise AssertionError("受限身份可读取管理员接口")
        except urllib.error.HTTPError as rejected:
            assert rejected.code == 403
        browser("set", "headers", json.dumps({"Authorization": "Bearer " + restricted}))
        browser("reload")
        browser("wait", "[data-app-settings-trigger]")
        browser("click", "[data-app-settings-trigger]")
        click("设置")
        browser("wait", "--text", "全局范围")
        browser("find", "role", "button", "click", "--name", "全局 全局范围", "--exact")
        browser("wait", "--text", "我的配置")
        browser("wait", "--fn", "![...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='CLI 管理')")
        browser("screenshot", str(evidence / "restricted.png"))
    finally:
        browser("set", "headers", "{}")
        if access_saved is None:
            access_file.unlink()
        else:
            access_file.write_bytes(access_saved)
    (evidence / "state.json").write_text(json.dumps(api("cli-management")["state"], ensure_ascii=False, indent=2), encoding="utf-8")
    print("通过：真实页面平台清单、安装/更新/修复/卸载、Actor 来源与外部文件保护、后台串行计划、记录/日志/归档、故障恢复、重启中断、三语主题与受限身份。真实供应商链路另行验收。")
finally:
    try:
        browser("snapshot", "-i")
        browser("close")
    finally:
        subprocess.run([str(binary), "daemon", "stop"], env=env, capture_output=True, timeout=20, check=False)
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.terminate()
            server.wait(timeout=10)
        output.close()
