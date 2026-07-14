# CLI Quick Start

## 1. Create And Select A Group

```bash
cd /path/to/project
cccc group create --title "Feature team" --topic "Ship the current milestone"
cccc groups
cccc group use <group_id> .
cccc active
```

## 2. Add Actors

```bash
cccc actor add foreman --runtime claude --runner pty
cccc actor add implementer --runtime codex --runner pty
cccc actor list
```

Use `--command` for a custom launch command. Use `cccc actor secrets` for private environment values instead of placing secrets in public actor configuration.

## 3. Start Work

```bash
cccc group start
cccc send "Break down the approved scope and assign one concrete task." --to foreman
cccc tracked-send "Implement the assigned task and reply with evidence." --to implementer
cccc tail -n 50
```

## 4. Read And Reply

```bash
cccc inbox --actor-id foreman --limit 20
cccc read <event_id> --actor-id foreman
cccc reply <event_id> "Acknowledged; continue with the validated approach."
```

## 5. Web And MCP

```bash
cccc setup
cccc
```

Open <http://127.0.0.1:8848>. `cccc mcp` starts the stdio MCP server directly.

## Runtime Modes

```bash
cccc runtime list
cccc actor add batch-worker --runtime custom --runner headless --command "your-agent-command"
cccc actor add web-agent --runtime web_model --runner headless
```

`web_model` actors use an actor-bound remote MCP connector and browser session. They do not launch a local child process.

## Integrations

```bash
cccc im status
cccc space status
cccc space bind --lane work
cccc space ingest --lane work --payload '{"title":"current context"}'
cccc space query "current context" --lane work
```

## Recovery

```bash
cccc doctor
cccc daemon status
cccc actor restart <actor_id>
cccc group stop
cccc group start
```

Use daemon restart only after actor/group recovery fails.

## Environment

```bash
export CCCC_HOME="$HOME/.cccc"
export CCCC_WEB_HOST=127.0.0.1
export CCCC_WEB_PORT=8848
```

Python and Rust use the same `CCCC_HOME`. Stop the active daemon before switching implementations.
