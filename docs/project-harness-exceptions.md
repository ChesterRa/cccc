# Frontend legacy file-budget register

Recorded 2026-11-14 for the remote-session/Direct release candidate, against
`origin/main` at `b4e861f928236ed8441ccf9c13800ecfd44f1fb8`.

These exceptions apply only to the existing files below. They do not relax the
220-line soft / 300-line hard limits for new files. Each recorded current line
count is a ceiling: no growth and no new responsibilities. New behavior must
live in a focused module before it is wired into these entries. Existing
components, hooks, services and stores remain in the established `web/src`
layout; this register does not require a new monorepo, overlay or CI.

## Existing files over 300 lines

Owner for every row: **CCCC frontend maintainers**; the release owner verifies
these ceilings before publishing. Counts include comments and blank lines.

| File | Baseline → current lines | Reason | Allowed scope | Expiry / removal condition | Split plan |
| --- | --- | --- | --- | --- | --- |
| `web/src/features/connect/DirectConnectionsPanel.test.tsx` | 555 → 541 | Existing interaction suite; rewriting its lifecycle would exceed this release's focused structural scope. Invitation/relation builders have been extracted without changing cases or assertions. | Necessary existing-case updates and helper extraction only; preserve isolation, assertions and test behavior. | Before the next change adds a Direct interaction scenario or grows this file. | Further move reusable DOM/lifecycle helpers to focused suite helpers; keep existing cases in place during this release. Future suite reorganization needs its own approved scope. |
| `web/src/features/localSessions/LocalSessionsButton.test.tsx` | 999 → 945 | Existing picker suite; interaction helpers were extracted. The existing empty-directory case now also verifies the merged device flow through real controls with network/clipboard boundaries; existing cases, visibility checks and admission assertions remain intact. T897 extends the existing unready-source case through focused control fixtures/actions without adding cases; the source-control suite also verifies partial readback, stale polls, revisions and exact heading focus. | Necessary existing-case updates and helper/fixture extraction only; do not weaken hidden-element checks or state assertions. | Before the next change adds a picker scenario or grows this file. | Continue separating picker actions and fixtures from mounting/lifecycle setup into focused helpers. Any future case-file split is outside this release's prohibition on new Web tests. |
| `web/src/features/runtimeSwitch/RuntimeSwitchControls.tsx` | 696 → 695 | Existing runtime-switch container; remote eligibility and current-directory derivation now delegate to `remoteSessionPolicy.ts`. A full form rewrite is outside this release. | Delegate remote session policy and retain existing load/save wiring; no additional business responsibility. | Before the next runtime-switch feature or any growth. | Separate native-source loading into a dedicated hook, then extract native/managed/Web form sections beside the existing container. |
| `web/src/hooks/useChatTab.ts` | 1533 → 1529 | Existing chat orchestration; setup and project-scope derivation now delegate to `chat/chatSetupRequirements.ts`. Unrelated composer/scroll refactoring is outside this release. | Delegate setup policy and preserve existing chat orchestration; no additional feature logic in this hook. | Before the next chat orchestration feature or any growth. | Continue existing `hooks/chat` decomposition: move setup, recipient derivation and composer lifecycle into focused hooks/helpers, leaving composition in `useChatTab`. |
| `web/src/components/messageBubble/PendingReplySlot.test.tsx` | 517 → 502 | Existing answer lifecycle suite. The scope-removal test now receives a valid matching success receipt; its request fixture was extracted to `PendingReplySlot.fixtures.ts` without adding cases. A full suite reorganization is outside this cleanup. | Correct existing protocol fixtures and extract helpers; preserve retry, scope isolation and receipt-validation assertions. | Before the next pending-answer scenario or any growth. | Move shared mounting/lifecycle and request/receipt fixtures into focused helpers; reorganize protocol and scope-isolation cases only in a separately approved scope. |

All five entries move toward decomposition and remain below their baseline line
counts. They are temporary, file-specific exceptions, not precedent for other
legacy files. No tests are added by this registration.

## Soft-limit split plan

`web/src/features/localSessions/useLocalSessionsConnection.ts`: **223 → 201 lines**;
owner **CCCC frontend maintainers**. T897 completed the source-poll extraction
into `useLocalSessionSources.ts`; mutations and controlled receive checks live in
`useLocalSourceControls.ts`. The existing facade composes these hooks and keeps
its selection/Group submission contract. It is now below the soft limit.

The remaining membership split plan is unchanged in intent: before the next
membership behavior expansion, extract `useTargetSessionMembership.ts` for target
Group polling, request generations, stale-result suppression and readiness.
Preserve cancellation and no-cache membership reads. No additional architecture,
state solution or test cases are introduced by this plan.

Two touched test suites also remain above the soft limit after cleanup:

- `web/tests/components/MarkdownRenderer.test.tsx`: **322 → 268 lines**.
  Owner: **CCCC frontend maintainers**. Before another renderer scenario is added,
  extract shared Mermaid mounting/preview lifecycle helpers, then separate diagram,
  table and link suites in an approved test-organization change. This cleanup
  removes source/CSS snapshots while retaining rendered output and interactions.
- `web/src/services/api/base.test.ts`: **267 → 250 lines**.
  Owner: **CCCC frontend maintainers**. Before another API scenario is added,
  separate HTTP response handling, authenticated URL behavior and data normalizers
  into focused suites. This cleanup removes only a duplicated normalizer case.
