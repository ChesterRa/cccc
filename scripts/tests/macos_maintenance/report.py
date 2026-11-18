"""Require positive evidence from each native case and publish a bounded summary."""
import json
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
names = (
    "native_watchdog_inherits_its_signal_mask_and_restores_the_callers_mask",
    "a_group_published_while_the_watchdog_is_paused_still_ends_with_its_owner",
    "self_check_reports_this_programs_constants",
    "system_browser_answers_cdp_and_stops_its_original_process",
    "failed_cdp_connection_stops_the_browser_opened_for_its_profile",
)
phases = (
    "previous_build", "inspector", "web_dependencies", "web_bundle", "candidate_build",
    "candidate_check", "candidate_clippy", "candidate_tests", "busy_browser", "daemon_keep", "local", "release", "lifecycle", "source_unchanged",
)

def read(name):
    p = root / name
    return p.read_text(errors="replace") if p.is_file() else ""

def document(name):
    try:
        return json.loads(read(name))
    except ValueError:
        return {}

rc = {phase: read(phase + ".rc").strip() or "not_run" for phase in phases}
log = read("candidate_tests.log")
tests = {name: len(re.findall(r"^test [^\n]*::" + name + r" \.\.\. ok$", log, re.M)) == 1 for name in names}
cases = {case: document(path) for case, path in (
    ("daemon_keep", "daemon_keep.json"), ("local", "local/result.json"), ("release", "release/result.json")
)}
busy = {mode: document("busy/" + mode + "/result.json") for mode in ("daemon","local","release")}
def busy_verified(mode, case):
    if not case or case.get("kind") != mode or not case.get("clean_stop"):
        return False
    events = case.get("events", [])
    timeline = [(e.get("event"), e.get("token"), e.get("attempts")) for e in events]
    return len({e.get("doc") for e in events}) == 1 and timeline == [
        ("open", "", 0), ("submit", "W2-ROUND-1", 1), ("finish", "W2-ROUND-1", 1),
        ("submit", "W2-ROUND-2", 2), ("finish", "W2-ROUND-2", 2),
    ]
busy_ok = {name: busy_verified(name, case) for name, case in busy.items()}
cap = document("candidate_check.log").get("capabilities", {})
ok = (all(x == "0" for x in rc.values()) and all(tests.values()) and all(busy_ok.values())
      and cap.get("local_restart") is True and cap.get("preserve_agents") is True
      and cases["daemon_keep"].get("normal_stop_clean") is True
      and all(cases[k].get("stopped_after_validation") is True for k in ("local", "release")))
result = {"passed": ok, "phases": rc, "required_tests": tests, "capabilities": cap,
          "scope": "disposable Apple Silicon candidate builds", "cases": cases, "busy_browser": busy_ok}
(root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
def notice(title, data):
    data = json.dumps(data, ensure_ascii=True).replace("%", "%25")
    print(f"::notice title={title}::{data}")
notice("macos-candidate-result", {"passed": ok, "phases": rc, "tests": tests, "capabilities": cap})
notice("macos-busy-browser", busy_ok)
for name, code in rc.items():
    if code not in ("0", "not_run"):
        diagnostic = read(name + ".log")
        if name == "candidate_clippy":
            rows = diagnostic.splitlines()
            diagnostic = "\n".join(line for i,line in enumerate(rows) if any(r.startswith("error") for r in rows[max(0,i-2):i+1]))
        notice("macos-failed-" + name, diagnostic[-2400:])
for name, case in cases.items():
    notice("macos-case-" + name, case)
sys.exit(0 if ok else 1)
