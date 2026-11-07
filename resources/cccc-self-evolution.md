# CCCC Self-Evolution

Capability ID: `skill:cccc:self-evolution`

A short review of how this team works, ending in at most three proposals for the user. Run it as the foreman when a task reminder or the user asks. `/cccc-self-evolution <focus>` narrows it to that focus. A peer asked to run it passes the foreman only what the foreman can already read; anything from a message addressed to that peer alone goes to the user.

## Review

1. Read what the user said lately: `cccc_message_history(mode="all", sender="user", limit=100)`. Page further with `before_event_id` only if the user asks for a wider look, and say how far back you read. You see only what reached you.
2. Read the numbers: `cccc_capability_use(tool_name="cccc_group", tool_arguments={"action":"activity"})`. Counts cover what you may read, and a scheduled notice is not proof of a model turn. They are leads, not verdicts: read what a number stands for before concluding anything about a member.
3. Check earlier changes first: page through the open `cccc_task` rows, pick those with `task_type` `optimization`, and read each by `task_id`. Was the change used in the next similar work? What happened, at what cost? If the user repeated the point, it failed. If nothing similar came up or the user said nothing, the result is unknown, not success. Write down what you know, and close the task once the outcome is clear.
4. Look for what the user had to say more than once, where work stalled or was redone, and where roles, members or settings do not fit the work.
5. Drop anything an existing rule, task or tool already covers.

## Propose

Send the user alone one question card per proposal, at most three, in the user's language. Each gives the evidence (the user's words with dates, or the numbers you read), the change, and how you would tell whether it worked. Leave out task status, secrets, rankings and unverified guesses. If nothing is worth the user's time, send nothing.

## Apply

Standing rules and changes to members wait for the user's pick:

- A standing rule goes into the brief's `constraints` through `cccc_coordination` `update_brief`. That call replaces the whole list, so read it first; bootstrap carries at most six. A long rule, or one about the project only, goes into the project's `AGENTS.md`.
- Record each adopted change as one `cccc_task` of type `optimization`: the evidence, the change, when it applies and how to undo it.

Routine division of work within your authority needs no card: change it and tell the user.

Tasks and constraints are visible to the whole Group. Keep out anything from a message addressed only to you, unless the user agrees.
