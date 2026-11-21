# Session owner contract 1 messages

`hello.json` and `snapshot.json` are a Hello and a Snapshot answer as a running
session owner of contract 1 wrote them on its control socket. The owner was
built from this repository before the first release with independent session
owners; `build.source_id` in `hello.json` names that build. It ran in a
disposable home without network access.

The snapshot has three members and one browser:

- `controlled`: a Codex member in a terminal, with an MCP binding that CCCC
  controls and one registered MCP process;
- `structured`: a Kimi member over ACP whose MCP binding has no registration;
- `terminal`: a custom command in a terminal;
- a system Chrome on a virtual display, with a VNC viewer and one login page
  that belongs to no member.

Changed from the capture: paths in the disposable home start with
`/tmp/fixture`, the DevTools WebSocket address is a placeholder, and the Linux
boot ID is zeros. Other PIDs, ports, IDs and times are as captured.

These files stay as contract 1 wrote them. When the contract changes, they
remain the sample of contract 1 instead of being regenerated.
