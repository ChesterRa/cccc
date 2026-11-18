#!/usr/bin/python3
"""Offline Hermes CLI contract: profile selection, `config path`, `config get`, `config unset`."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ["CCCC_HERMES_REGISTRATION_FIXTURE_ROOT"])
argv = sys.argv[1:]
with (root / "hermes-calls.jsonl").open("a") as log:
    log.write(json.dumps(argv) + "\n")
if argv == ["--version"]:
    print("Owned Hermes fixture")
    sys.exit(0)

home = Path(os.environ["HERMES_HOME"])
# A HERMES_HOME that names a profile directory is used as it is.
in_profile = home.parent.name == "profiles"
base = home.parent.parent if in_profile else home
profile = None
explicit = argv[:1] == ["--profile"]
if explicit:
    profile, argv = argv[1], argv[2:]
    directory = base if profile == "default" else base / "profiles" / profile
elif in_profile:
    directory = home
else:
    # Without --profile every command rereads the sticky profile.
    sticky = base / "active_profile"
    if sticky.exists() and sticky.read_text().strip() not in ("", "default"):
        profile = sticky.read_text().strip()
    directory = base / "profiles" / profile if profile else base
if not directory.is_dir():
    if explicit:
        print(f"Error: Profile '{profile}' does not exist.", file=sys.stderr)
    else:
        print(f"Error: Saved profile '{profile}' no longer exists.", file=sys.stderr)
    sys.exit(1)
config_path = directory / "config.yaml"
managed = base / "managed.json"


def load(path):
    """The user's file; a broken one leaves Hermes on its last good copy."""
    try:
        return json.loads(path.read_text()) if path.exists() else {}
    except ValueError:
        good = path.parent / "last-good.json"
        return json.loads(good.read_text()) if good.exists() else {}


def managed_section(section):
    return (json.loads(managed.read_text()).get(section) or {}) if managed.exists() else {}


if argv == ["config", "path"]:
    if explicit and (root / "pinned-path-differs").exists():
        print(root / "elsewhere" / "config.yaml")
        sys.exit(0)
    print(config_path)
    switch = root / "switch-sticky-after-path"
    if not explicit and switch.exists():
        (base / "active_profile").write_text(switch.read_text())
        switch.unlink()
    sys.exit(0)

if argv[:2] == ["config", "get"] and argv[3:] == ["--json"]:
    section, _, name = argv[2].partition(".")
    servers = dict(load(config_path).get(section) or {})
    servers.update(managed_section(section))
    if name not in servers:
        print(f"Config key not set: {argv[2]}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps(servers[name]))
    sys.exit(0)

if argv[:2] == ["config", "unset"] and len(argv) == 3:
    section, _, name = argv[2].partition(".")
    if name in managed_section(section):
        print(f"Cannot unset '{argv[2]}': it is managed by your administrator.", file=sys.stderr)
        sys.exit(1)
    config = json.loads(config_path.read_text()) if config_path.exists() else {}
    servers = config.get(section) or {}
    if name not in servers:
        print(f"Config key not set: {argv[2]}", file=sys.stderr)
        sys.exit(1)
    if not (root / "remove-ignored").exists():
        del servers[name]
        if servers:
            config[section] = servers
        else:
            config.pop(section, None)
        config_path.write_text(json.dumps(config))
    print(f"Unset {argv[2]} from {config_path}")
    sys.exit(0)

print("unsupported fixture command: " + " ".join(argv), file=sys.stderr)
sys.exit(2)
