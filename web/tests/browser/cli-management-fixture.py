#!/usr/bin/env python3
"""仅供 CLI 管理真实 Web 验收使用的跨平台 mise 替身；不访问供应商。"""
import json
import os
from pathlib import Path
import shutil
import sys
import time

root = Path(sys.argv[1]).resolve()
if (root / "fixture-marker").read_text(encoding="utf-8") != "controlled-cli-only":
    raise SystemExit("拒绝在非测试目录执行")
command = sys.argv[2]
if command == "latest":
    print((root / "target-version").read_text(encoding="utf-8"))
elif command == "env":
    print(json.dumps({"PATH": str(Path(os.environ["MISE_DATA_DIR"]) / "bin")}))
elif command == "install":
    version = (root / "target-version").read_text(encoding="utf-8").strip()
    while (root / "hold-install").exists():
        time.sleep(0.2)
    time.sleep(1)
    destination = Path(os.environ["MISE_DATA_DIR"]) / "bin"
    destination.mkdir(parents=True, exist_ok=True)
    for argument in sys.argv[3:]:
        name = argument.split("@", 1)[0]
        if name not in ("grok", "codex", "antigravity-cli"):
            raise SystemExit("仅允许受控 Grok/Codex/Antigravity 安装")
        if name == "antigravity-cli":
            name = "agy"
        source = root / "releases" / version
        target = destination / (name + (".cmd" if os.name == "nt" else ""))
        shutil.copyfile(source, target)
        if os.name != "nt":
            target.chmod(0o700)
    print("受控安装完成")
    print("token=web-fixture-secret", file=sys.stderr)
else:
    raise SystemExit(f"未知测试命令：{command}")
