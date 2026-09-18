# Web UI Quick Start

Use the Web interface to create a working Group, start Agents and follow their work.

## 1. Open CCCC

Run `cccc`, then open [http://127.0.0.1:8848/](http://127.0.0.1:8848/).
This starts the daemon and Web interface. Keep the terminal running.

## 2. Create a Group

Click **+ New** in the sidebar. Choose a project directory on the machine running
CCCC, give the Group a name, and create it. **Browse** lets you select a directory
without typing its full path. Group administration requires administrator access.

Alternatively, attach the current directory from a terminal:

```bash
cd /path/to/your/project
cccc attach .
```

Select the Group in the sidebar. Its name and run status appear in the header.
The pencil beside the name edits the Group's name and description.

## 3. Add an Agent

Use the **+** in the Agent bar near the composer. Choose an installed Runtime
(for example, Claude Code or Codex), set an Actor ID and review the configuration.
Add the Agent when ready. The first Group coordinator has the **foreman** role;
additional Agents can work as peers.

Complete the Runtime's own sign-in/setup if needed. To check its CCCC integration:

```bash
cccc setup --runtime claude
```

Use the corresponding Runtime name for other CLIs. Managed Claude Code, Codex,
Grok Build and OpenCode sessions receive their scoped CCCC MCP entry at startup.

## 4. Start and communicate

Open the Group status button in the header and choose **Start** to launch enabled
Agents. To inspect or control one Agent, open it from the Agent bar.

In **Messages**, choose recipients using the **To** controls above the input,
write a message and click **Send** or press `Ctrl+Enter` / `Cmd+Enter`.
Choose an individual Agent or `@foreman` for directed work; use `@all` when every
enabled Agent needs the message.

Typing `@` or `#` inserts references into the text. References do not replace the
recipient controls. A selected remote `#Group` includes its Connect identity for
your local Agents to use; it does not directly send a remote message.

Switch to **Terminals** to see several Agents at once. The page arrows show the
remaining Agents. Return to **Messages** to read replies or continue the discussion.

## 5. Keep the work organized

- **Project Context** (clipboard button in the header): **Coordination** contains
  the working summary, `PROJECT.md`, coordination log and task board. **Agent State**
  shows each Agent's latest saved report; it is not a live process monitor.
  **Self-Evolving Skills** shows this Group's generated skill candidates.
- **Files**: browse the workspace, preview supported files and inspect changes.
  Desktop editing and file operations are available subject to your access.
- **Presentation**: pin up to four resources for quick access and reading.
- **Settings and more**: adjust appearance or open settings. Select **This group**
  or **This instance** before changing configuration.

## Common controls

| Action | Control |
| --- | --- |
| Switch Group | Sidebar Group list |
| Edit Group name | Pencil beside the current Group name |
| Start / pause delivery / stop Group | Group status button |
| Add or inspect an Agent | Agent bar near the composer |
| Search messages | Search button in the Group header |
| Reply | **Reply** below a message |
| Send | **Send**, `Ctrl+Enter` or `Cmd+Enter` |
| Insert a line break | `Enter` |
| Mention suggestions | Type `@` or `#`; use arrows, then `Tab` / `Enter` |
| Close the current menu/dialog | `Escape` |

## Troubleshooting

**The page does not load:** check `cccc daemon status` and the terminal running
CCCC. To use another Web port, run `CCCC_WEB_PORT=9000 cccc`.

**An Agent does not start:** open its Runtime inspector, read the reported error,
check that the CLI is installed and signed in, and run
`cccc setup --runtime <name>` if the integration needs attention.

**The project is not listed:** create a Group or run `cccc attach .` in its
project directory. Paths refer to the CCCC host, not the device displaying the
browser. Use an absolute path or one starting with `~`; Browse can help find it.

## Next steps

- [Workflows](/guide/workflows)
- [Web UI Guide](/guide/web-ui)
- [Voice Secretary](/guide/voice-secretary)
- [IM Bridge](/guide/im-bridge/)
