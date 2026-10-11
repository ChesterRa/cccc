#!/usr/bin/python3
"""Offline Hermes add contract: REMAINDER arguments and discovery-first confirmations."""
import argparse
import json
import os
from pathlib import Path
import sys

if sys.argv[1:] == ["--version"]:
    print("Owned Hermes fixture")
    sys.exit(0)

parser = argparse.ArgumentParser()
parser.add_argument("command", choices=["mcp"])
parser.add_argument("action", choices=["add"])
parser.add_argument("name")
parser.add_argument("--command", dest="mcp_command")
parser.add_argument("--args", nargs=argparse.REMAINDER, default=[])
parser.add_argument("--env", nargs="*", default=[])
args = parser.parse_args()

profile = Path(os.environ["HERMES_HOME"])
config_path = profile / "config.yaml"
config = json.loads(config_path.read_text())


def confirm(question, default=False):
    try:
        value = input(question).strip().lower()
    except EOFError:
        return default
    return value in ("y", "yes") if value else default


def save(entry):
    config.setdefault("mcp_servers", {})[args.name] = entry
    config_path.write_text(json.dumps(config))


if args.name in config.get("mcp_servers", {}):
    if not confirm("Server already exists. Overwrite? [y/N]: "):
        sys.exit(0)

entry = {"command": args.mcp_command}
if args.args:
    entry["args"] = args.args
if args.env:
    entry["env"] = dict(value.split("=", 1) for value in args.env)

failed = args.args != ["mcp"] or (Path(os.environ["CCCC_HERMES_REGISTRATION_FIXTURE_ROOT"]) / "probe-fails").exists()
if failed:
    print("Failed to connect: owned fixture rejection")
    if confirm("Save config anyway (you can test later)? [y/N]: "):
        entry["enabled"] = False
        save(entry)
    sys.exit(0)

try:
    choice = input("Enable all 1 tools? [Y/n/select]: ").strip().lower()
except EOFError:
    sys.exit(0)
if choice in ("n", "no"):
    sys.exit(0)
entry["enabled"] = True
save(entry)
