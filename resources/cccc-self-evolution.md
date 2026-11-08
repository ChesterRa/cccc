# CCCC Self-Evolution

Capability ID: `skill:cccc:self-evolution`

A short review of how this team works, ending in at most three proposals for the user. Run it as the foreman when a task reminder or the user asks. `/cccc-self-evolution <focus>` narrows it to that focus. A peer asked to run it passes the foreman only what the foreman can already read, or what the user authorized and the team's work needs, while honoring confidentiality limits.

## Review

1. Read the user's last 100 messages in two pages: `cccc_message_history(mode="all", sender="user", limit=50)`. If `has_more` is true, repeat with `before_event_id` set to the `id` of the last message returned, which is the oldest. Go further back only if the user asks, and say how far you read. You see only what reached you.
2. Read the numbers: `cccc_capability_use(tool_name="cccc_group", tool_arguments={"action":"activity"})`. Message counts cover only what you may read. Scheduled notices cover the whole Group and prove neither delivery nor a model turn. All of them are leads, not verdicts: read what a number stands for before concluding anything about a member.
3. Check earlier changes first: page through the open `cccc_task` rows, pick those with `task_type` `optimization`, and read each by `task_id`. Was the change used in the next similar work? What happened, at what cost? If the user had to repeat the point, it failed. If nothing similar came up, the result is unknown; the user saying nothing does not prove it worked, so record what the team can check directly. A question marked `Review:`, in a task's title or at the start of a line in its notes, was handed over for a deeper look, may point to another task or handoff, and authorizes no change: turn it into a proposal, resolve it with the reason, or leave it open and say what it waits for. Add what you know to the notes, keeping the existing notes and passing the `if_task_rev` you read, and close the task once its own outcome is clear and no `Review:` question in it is still open. A rule that applied in real cases without the intended effect, shown by evidence, may be revised or proposed for removal; do not remove one only because nothing recurred or the user said nothing. One a tool or check could enforce may be worth making into code.
4. Look for what the user had to say more than once, where work stalled or was redone, and where roles, members or settings do not fit the work.
5. Drop what an existing rule, task or tool already handles, unless, after it existed, the user still corrected the same thing or kept asking the same question, or the team saw the same failure again. Then find out why and propose a fix.

## Propose

Send the user alone one question card per proposal, at most three, in the user's language. Each gives the evidence (the user's words with dates, or the numbers you read), the change, and how you would tell whether it worked. Leave out task status, secrets, rankings and unverified guesses. Do not resend a card the user has not answered. If nothing is worth the user's time, send nothing; when the user asked for this review, say briefly what you read and that nothing needed their decision.

## Apply

Standing rules, their removal and changes to members wait for the user's pick:

- A standing rule goes into the brief's `constraints` through `cccc_coordination` `update_brief`. That call replaces the whole list, so read it first; bootstrap carries at most six. A long rule, or one about the project only, goes into the project's `AGENTS.md`.
- Record each adopted change, and each change within your authority worth checking later, as one `cccc_task` of type `optimization`: the evidence, the change, when it applies, what to look at next time and how to undo it. A one-off fix needs none.

Routine division of work within your authority needs no card: change it and tell the user.

Tasks and constraints are visible to the whole Group. For a message addressed only to you, share only what the user authorized and the team's work needs, in your own words, and honor any confidentiality limits. Do not include its ID unless the user specifically agreed.
