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
    "candidate_check", "candidate_tests", "daemon_keep", "local", "release", "lifecycle", "source_unchanged",
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
cap = document("candidate_check.log").get("capabilities", {})
ok = (all(x == "0" for x in rc.values()) and all(tests.values())
      and cap.get("local_restart") is True and cap.get("preserve_agents") is True
      and cases["daemon_keep"].get("normal_stop_clean") is True
      and all(cases[k].get("stopped_after_validation") is True for k in ("local", "release")))
result = {"passed": ok, "phases": rc, "required_tests": tests, "capabilities": cap,
          "scope": "disposable Apple Silicon candidate builds", "cases": cases}
(root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
def notice(title, data):
    data = json.dumps(data, ensure_ascii=True).replace("%", "%25")
    print(f"::notice title={title}::{data}")
notice("macos-candidate-result", {"passed": ok, "phases": rc, "tests": tests, "capabilities": cap})
for name, code in rc.items():
    if code not in ("0", "not_run"):
        notice("macos-failed-" + name, read(name + ".log")[-1400:])
for name, case in cases.items():
    notice("macos-case-" + name, case)
sys.exit(0 if ok else 1)
