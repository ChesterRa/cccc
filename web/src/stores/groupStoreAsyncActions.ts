import type { GroupContext, GroupDoc, LedgerEvent } from "../types";
import * as api from "../services/api";
import { mergeLedgerEvents } from "../utils/mergeLedgerEvents";
import {
  beginContextRequest,
  beginGroupRequestEpoch,
  buildChatBucketPatch,
  buildShellGroupDoc,
  buildPrimedGroupState,
  EMPTY_CHAT_BUCKET,
  ensureGroupChatBucket,
  filterUiEvents,
  getCachedGroupView,
  getGroupChatBucket,
  groupViewCache,
  type GroupChatBucket,
  type GroupViewSnapshot,
  INITIAL_LEDGER_TAIL_LIMIT,
  internalActorsRequestEpochByGroup,
  isLatestContextRequest,
  isLatestGroupRequestEpoch,
  loadGroupInFlight,
  loadGroupToken,
  loadSelectedGroupId,
  MAX_UI_EVENTS,
  mergeActorUnreadCounts,
  patchGroupRuntimeStatus,
  presentationRequestEpochByGroup,
  refreshActorsInFlight,
  refreshActorsQueued,
  refreshGroupsInFlight,
  refreshGroupsQueued,
  saveGroupView,
  saveSelectedGroupId,
  scheduleDeferredUnreadRefresh,
  setRefreshGroupsInFlight,
  setRefreshGroupsQueued,
  settingsRequestEpochByGroup,
  warmGroupInFlight,
  incrementLoadGroupToken,
  chatWindowRequestEpochByGroup,
  deriveRuntimeStatusFromActors,
} from "./groupStoreCore";
import type { GroupStoreAsyncActions, GroupStoreGet, GroupStoreSet, GroupState } from "./groupStoreTypes";
import { useComposerStore } from "./useComposerStore";

export function createGroupStoreAsyncActions(
  set: GroupStoreSet,
  get: GroupStoreGet,
): GroupStoreAsyncActions {
  return {
    refreshGroups: async () => {
      if (refreshGroupsInFlight) {
        setRefreshGroupsQueued(true);
        return;
      }
      setRefreshGroupsInFlight(true);
      try {
        const resp = await api.fetchGroups();
        if (resp.ok) {
          const next = resp.result.groups || [];
          get().setGroups(next);

          const cur = String(get().selectedGroupId || "").trim();
          const curExists = !!cur && next.some((g) => String(g.group_id || "") === cur);
          if (!curExists) {
            if (next.length > 0) {
              const persisted = loadSelectedGroupId();
              const persistedExists = !!persisted && next.some((g) => String(g.group_id || "") === persisted);
              const nextGroupId = persistedExists ? persisted : String(next[0].group_id || "");
              get().setSelectedGroupId(nextGroupId);
            } else {
              groupViewCache.clear();
              get().setSelectedGroupId("");
            }
            return;
          }

          if (String(useComposerStore.getState().activeGroupId || "").trim() !== cur) {
            get().setSelectedGroupId(cur);
          } else {
            saveSelectedGroupId(cur);
          }

          const selectedId = get().selectedGroupId;
          const doc = get().groupDoc;
          if (doc && selectedId && String(doc.group_id || "") === String(selectedId || "")) {
            const meta = next.find((g) => String(g.group_id || "") === String(selectedId || "")) || null;
            if (meta) {
              const patch: Partial<GroupDoc> = {};
              if (typeof meta.state === "string" && meta.state !== doc.state) patch.state = meta.state;
              if (typeof meta.title === "string" && meta.title !== doc.title) patch.title = meta.title;
              if (typeof meta.topic === "string" && meta.topic !== doc.topic) patch.topic = meta.topic;
              if (typeof meta.running === "boolean" && meta.running !== doc.running) patch.running = meta.running;
              if (meta.runtime_status) {
                const curRT = doc.runtime_status;
                if (
                  !curRT
                  || curRT.lifecycle_state !== meta.runtime_status.lifecycle_state
                  || curRT.runtime_running !== meta.runtime_status.runtime_running
                  || curRT.running_actor_count !== meta.runtime_status.running_actor_count
                ) {
                  patch.runtime_status = meta.runtime_status;
                }
              }
              if (Object.keys(patch).length > 0) {
                const nextDoc = { ...doc, ...patch };
                set({ groupDoc: nextDoc });
                saveGroupView(selectedId, { groupDoc: nextDoc });
              }
            }
          }
        }
      } catch (e) {
        console.error("Failed to refresh groups:", e);
      } finally {
        setRefreshGroupsInFlight(false);
        if (refreshGroupsQueued) {
          setRefreshGroupsQueued(false);
          void get().refreshGroups();
        }
      }
    },

    refreshActors: async (groupId?: string, opts?: { includeUnread?: boolean }) => {
      const gid = String(groupId || get().selectedGroupId || "").trim();
      if (!gid) return;
      const includeUnread = opts?.includeUnread !== false;
      if (refreshActorsInFlight.has(gid)) {
        const queuedIncludeUnread = refreshActorsQueued.get(gid) ?? false;
        refreshActorsQueued.set(gid, queuedIncludeUnread || includeUnread);
        return;
      }
      refreshActorsInFlight.add(gid);
      try {
        const resp = await api.fetchActors(gid, includeUnread);
        if (resp.ok) {
          const prevActors = get().selectedGroupId === gid ? get().actors : getCachedGroupView(gid)?.actors || [];
          const nextActors = includeUnread
            ? (resp.result.actors || [])
            : mergeActorUnreadCounts(resp.result.actors || [], prevActors);
          const current = get();
          const runtimeFallback =
            current.groupDoc?.group_id === gid
              ? current.groupDoc.runtime_status || null
              : current.groups.find((group) => String(group.group_id || "").trim() === gid)?.runtime_status || null;
          const runtimeStatus = deriveRuntimeStatusFromActors(nextActors, runtimeFallback);
          const nextGroups = patchGroupRuntimeStatus(current.groups, gid, runtimeStatus);
          const nextGroupDoc = current.selectedGroupId === gid && current.groupDoc
            ? {
                ...current.groupDoc,
                running: runtimeStatus.runtime_running,
                state: runtimeStatus.lifecycle_state as GroupDoc["state"],
                runtime_status: runtimeStatus,
              }
            : undefined;
          saveGroupView(gid, { actors: nextActors, groupDoc: nextGroupDoc });
          const patch: Partial<GroupState> = {};
          if (nextGroups !== current.groups) patch.groups = nextGroups;
          if (current.selectedGroupId === gid) {
            patch.actors = nextActors;
            patch.selectedGroupActorsHydrating = false;
            if (nextGroupDoc) patch.groupDoc = nextGroupDoc;
          }
          if (Object.keys(patch).length > 0) {
            set(patch);
          }
        }
      } catch (e) {
        console.error(`Failed to refresh actors for group=${gid}:`, e);
      } finally {
        refreshActorsInFlight.delete(gid);
        const queuedIncludeUnread = refreshActorsQueued.get(gid);
        if (queuedIncludeUnread !== undefined) {
          refreshActorsQueued.delete(gid);
          void get().refreshActors(gid, { includeUnread: queuedIncludeUnread });
        }
      }
    },

    refreshInternalRuntimeActors: async (groupId?: string) => {
      const gid = String(groupId || get().selectedGroupId || "").trim();
      if (!gid) return;
      const epoch = beginGroupRequestEpoch(internalActorsRequestEpochByGroup, gid);
      const inFlightKey = `internal:${gid}`;
      if (refreshActorsInFlight.has(inFlightKey)) return;
      refreshActorsInFlight.add(inFlightKey);
      try {
        const resp = await api.fetchActors(gid, false, { noCache: true }, { includeInternal: true });
        if (!resp.ok) return;
        if (!isLatestGroupRequestEpoch(internalActorsRequestEpochByGroup, gid, epoch)) return;
        const nextActors = (resp.result.actors || []).filter((actor) => {
          const internalKind = String(actor.internal_kind || "").trim().toLowerCase();
          return internalKind === "pet" || internalKind === "voice_secretary";
        });
        set((state) => ({
          internalRuntimeActorsByGroup: {
            ...state.internalRuntimeActorsByGroup,
            [gid]: nextActors,
          },
        }));
      } catch (e) {
        console.error(`Failed to refresh internal runtime actors for group=${gid}:`, e);
      } finally {
        refreshActorsInFlight.delete(inFlightKey);
      }
    },

    refreshSettings: async (groupId?: string) => {
      const gid = String(groupId || get().selectedGroupId || "").trim();
      if (!gid) return;
      const epoch = beginGroupRequestEpoch(settingsRequestEpochByGroup, gid);
      try {
        const resp = await api.fetchSettings(gid);
        if (!resp.ok) return;
        if (!isLatestGroupRequestEpoch(settingsRequestEpochByGroup, gid, epoch)) return;
        const nextSettings = resp.result.settings || null;
        saveGroupView(gid, { groupSettings: nextSettings });
        if (String(get().selectedGroupId || "").trim() === gid) {
          set({ groupSettings: nextSettings });
        }
      } catch (error) {
        console.error(`Failed to refresh settings for group=${gid}:`, error);
      }
    },

    refreshPresentation: async (groupId?: string) => {
      const gid = String(groupId || get().selectedGroupId || "").trim();
      if (!gid) return;
      const epoch = beginGroupRequestEpoch(presentationRequestEpochByGroup, gid);
      try {
        const resp = await api.fetchPresentation(gid);
        if (!resp.ok) return;
        if (!isLatestGroupRequestEpoch(presentationRequestEpochByGroup, gid, epoch)) return;
        const nextPresentation = resp.result.presentation || null;
        saveGroupView(gid, { groupPresentation: nextPresentation });
        if (String(get().selectedGroupId || "").trim() === gid) {
          set({ groupPresentation: nextPresentation });
        }
      } catch (error) {
        console.error(`Failed to refresh presentation for group=${gid}:`, error);
      }
    },

    loadGroup: async (groupId: string) => {
      const gid = String(groupId || "").trim();
      if (!gid) return;
      const inFlight = loadGroupInFlight.get(gid);
      if (inFlight) return;

      const token = incrementLoadGroupToken();
      const isLatestSelection = () => get().selectedGroupId === gid && loadGroupToken === token;
      const commitViewPatch = (
        patch: Partial<Pick<GroupState, "groupDoc" | "actors" | "groupContext" | "groupSettings" | "groupPresentation">>,
      ) => {
        saveGroupView(gid, patch);
        if (isLatestSelection()) {
          set(patch);
        }
      };
      const commitChatPatch = (patch: Partial<GroupChatBucket>) => {
        const cachePatch: Partial<Omit<GroupViewSnapshot, "cachedAt">> = {};
        if (patch.events !== undefined) cachePatch.events = patch.events;
        if (patch.hasMoreHistory !== undefined) cachePatch.hasMoreHistory = patch.hasMoreHistory;
        if (Object.keys(cachePatch).length > 0) {
          saveGroupView(gid, cachePatch);
        }
        if (isLatestSelection()) {
          set((state) => buildChatBucketPatch(state, gid, patch) ?? state);
        }
      };
      const state = get();
      const currentDocGroupId = String(state.groupDoc?.group_id || "").trim();
      if (currentDocGroupId !== gid) {
        const nextChatByGroup = ensureGroupChatBucket(state.chatByGroup, gid);
        const chatBucket = nextChatByGroup[gid] || EMPTY_CHAT_BUCKET;
        const primedState = buildPrimedGroupState(gid, state.groups);
        saveGroupView(gid, {
          groupDoc: primedState.groupDoc,
          events: chatBucket.events,
          actors: primedState.actors,
          groupContext: primedState.groupContext,
          groupSettings: primedState.groupSettings,
          groupPresentation: primedState.groupPresentation,
          hasMoreHistory: chatBucket.hasMoreHistory,
        });
        if (isLatestSelection()) {
          set({ chatByGroup: nextChatByGroup, selectedGroupActorsHydrating: true, ...primedState });
        }
      }

      const hydrateTailStatuses = async (events: LedgerEvent[]) => {
        const eventIds = events
          .filter((event) => event.kind === "chat.message")
          .map((event) => String(event.id || "").trim())
          .filter((eventId) => eventId);
        if (eventIds.length === 0) return;
        const statusesResp = await api.fetchLedgerStatuses(gid, eventIds);
        if (!statusesResp.ok || !isLatestSelection()) return;
        get().mergeEventStatuses(statusesResp.result.statuses || {}, gid);
      };

      const showPromise = api.fetchGroup(gid);
      const tailPromise = api.fetchLedgerTail(gid, INITIAL_LEDGER_TAIL_LIMIT, { includeStatuses: false });
      const actorsPromise = api.fetchActors(gid, false);
      const contextEpoch = beginContextRequest(gid);
      const settingsEpoch = beginGroupRequestEpoch(settingsRequestEpochByGroup, gid);

      void showPromise.then((show) => {
        if (show.ok) {
          commitViewPatch({ groupDoc: show.result.group });
          return;
        }

        const code = String(show.error?.code || "").trim();
        if (code === "group_not_found") {
          groupViewCache.delete(gid);
          set((state) => ({
            ...(buildChatBucketPatch(state, gid, {
              events: [],
              chatWindow: null,
              hasMoreHistory: false,
              hasLoadedTail: true,
              isLoadingHistory: false,
              isChatWindowLoading: false,
            }) || {}),
            ...(isLatestSelection()
              ? {
                  groupDoc: null,
                  actors: [],
                  groupContext: null,
                  groupSettings: null,
                  groupPresentation: null,
                }
              : {}),
          }));
        }
      }).catch((error) => {
        console.error(`Failed to load group metadata for group=${gid}:`, error);
      });

      void tailPromise.then((tail) => {
        if (!tail.ok) {
          commitChatPatch({ hasLoadedTail: true });
          return;
        }
        const mergedEvents = mergeLedgerEvents(
          getGroupChatBucket(get().chatByGroup, gid).events,
          filterUiEvents(tail.result.events || []),
          MAX_UI_EVENTS,
        );
        commitChatPatch({
          events: mergedEvents,
          hasMoreHistory: !!tail.result.has_more,
          hasLoadedTail: true,
        });
        void hydrateTailStatuses(mergedEvents).catch((error) => {
          console.error(`Failed to hydrate ledger statuses for group=${gid}:`, error);
        });
      }).catch((error) => {
        console.error(`Failed to load ledger tail for group=${gid}:`, error);
        commitChatPatch({ hasLoadedTail: true });
      });

      void actorsPromise.then((actorsResp) => {
        if (!actorsResp.ok) return;
        commitViewPatch({ actors: actorsResp.result.actors || [] });
      }).catch((error) => {
        console.error(`Failed to load actors for group=${gid}:`, error);
      }).finally(() => {
        if (isLatestSelection()) {
          set({ selectedGroupActorsHydrating: false });
        }
      }).finally(() => {
        scheduleDeferredUnreadRefresh(gid, () => {
          if (isLatestSelection()) {
            void get().refreshActors(gid, { includeUnread: true });
          }
        });
      });

      void api.fetchContext(gid, { detail: "summary" }).then((ctx) => {
        if (!ctx.ok) return;
        if (!isLatestContextRequest(gid, contextEpoch)) return;
        const summary = ctx.result as GroupContext;
        const meta = summary && typeof summary === "object" ? (summary as { meta?: unknown }).meta : null;
        const summarySnapshot = meta && typeof meta === "object"
          ? ((meta as { summary_snapshot?: unknown }).summary_snapshot ?? null)
          : null;
        const snapshotState = summarySnapshot && typeof summarySnapshot === "object"
          ? String((summarySnapshot as { state?: unknown }).state || "").trim().toLowerCase()
          : "";
        const currentState = get();
        const hasCachedContext =
          String(currentState.groupDoc?.group_id || "").trim() === gid &&
          currentState.groupContext !== null;

        if (snapshotState === "stale" && hasCachedContext) return;
        if (snapshotState === "missing" || (snapshotState === "stale" && !hasCachedContext)) {
          void api.fetchContext(gid, { detail: "full", fresh: true }).then((fullCtx) => {
            if (!fullCtx.ok) return;
            if (!isLatestContextRequest(gid, contextEpoch)) return;
            commitViewPatch({ groupContext: fullCtx.result as GroupContext });
          }).catch((error) => {
            console.error(`Failed to load fresh context for group=${gid}:`, error);
          });
          return;
        }

        commitViewPatch({ groupContext: summary });
      }).catch((error) => {
        console.error(`Failed to load context for group=${gid}:`, error);
      });

      void api.fetchSettings(gid).then((settings) => {
        if (!settings.ok) return;
        if (!isLatestGroupRequestEpoch(settingsRequestEpochByGroup, gid, settingsEpoch)) return;
        commitViewPatch({ groupSettings: settings.result.settings || null });
      }).catch((error) => {
        console.error(`Failed to load settings for group=${gid}:`, error);
      });

      const initialLoad = Promise.allSettled([showPromise, tailPromise, actorsPromise]);
      const initialLoadDone = initialLoad.then(() => undefined);
      loadGroupInFlight.set(gid, initialLoadDone);
      void initialLoad.then(() => {
        const timeout = window.setTimeout(() => {
          const presentationEpoch = beginGroupRequestEpoch(presentationRequestEpochByGroup, gid);
          void api.fetchPresentation(gid).then((presentationResp) => {
            if (!presentationResp.ok) return;
            if (!isLatestGroupRequestEpoch(presentationRequestEpochByGroup, gid, presentationEpoch)) return;
            commitViewPatch({ groupPresentation: presentationResp.result.presentation || null });
          }).catch((error) => {
            console.error(`Failed to load presentation for group=${gid}:`, error);
          });
        }, 250);
        if (!isLatestSelection()) {
          window.clearTimeout(timeout);
        }
      }).finally(() => {
        if (loadGroupInFlight.get(gid) === initialLoadDone) {
          loadGroupInFlight.delete(gid);
        }
      });
    },

    warmGroup: async (groupId: string) => {
      const gid = String(groupId || "").trim();
      if (!gid) return;
      if (gid === String(get().selectedGroupId || "").trim()) return;
      if (warmGroupInFlight.has(gid)) return;
      if (getCachedGroupView(gid)) return;

      warmGroupInFlight.add(gid);
      try {
        const [show, tail, actorsResp] = await Promise.all([
          api.fetchGroup(gid),
          api.fetchLedgerTail(gid, INITIAL_LEDGER_TAIL_LIMIT, { includeStatuses: false }),
          api.fetchActors(gid, false),
        ]);

        const patch: Partial<Omit<GroupViewSnapshot, "cachedAt">> = {};
        if (show.ok) {
          patch.groupDoc = show.result.group;
        } else {
          patch.groupDoc = buildShellGroupDoc(gid, get().groups, null);
        }
        if (tail.ok) {
          patch.events = filterUiEvents(tail.result.events || []);
          patch.hasMoreHistory = !!tail.result.has_more;
        }
        if (actorsResp.ok) {
          patch.actors = actorsResp.result.actors || [];
        }
        saveGroupView(gid, patch);
      } catch (error) {
        console.error(`Failed to warm group=${gid}:`, error);
      } finally {
        warmGroupInFlight.delete(gid);
      }
    },

    loadMoreHistory: async (groupId?: string) => {
      const gid = String(groupId || get().selectedGroupId || "").trim();
      if (!gid) return;

      const bucket = getGroupChatBucket(get().chatByGroup, gid);
      if (bucket.isLoadingHistory || !bucket.hasMoreHistory) return;

      const chatMessages = bucket.events.filter((event) => event.kind === "chat.message");
      const firstEvent = chatMessages[0];
      if (!firstEvent?.id) return;

      set((state) => buildChatBucketPatch(state, gid, { isLoadingHistory: true }) ?? state);
      try {
        const resp = await api.fetchOlderMessages(gid, String(firstEvent.id), 50);
        if (resp.ok) {
          const olderChatEvents = (resp.result.events || []).filter(
            (event) => event && event.kind === "chat.message",
          );
          const currentBucket = getGroupChatBucket(get().chatByGroup, gid);
          const existingIds = new Set(currentBucket.events.map((event) => event.id).filter(Boolean));
          const uniqueNew = olderChatEvents.filter((event) => event.id && !existingIds.has(event.id));
          const exhaustedHistory = olderChatEvents.length === 0 || uniqueNew.length === 0;
          const merged = [...uniqueNew, ...currentBucket.events];
          // Atomic update: merge events + clear isLoadingHistory in a single set()
          // to avoid an intermediate rerender where events changed but isLoadingHistory
          // is still true, which would cause useLayoutEffect to skip scroll compensation
          // while the virtualizer already re-measured with the new (prepended) messages.
          set((state) => buildChatBucketPatch(state, gid, {
            events: merged.length > MAX_UI_EVENTS ? merged.slice(0, MAX_UI_EVENTS) : merged,
            hasMoreHistory: exhaustedHistory ? false : !!resp.result.has_more,
            isLoadingHistory: false,
          }) ?? state);
        } else {
          set((state) => buildChatBucketPatch(state, gid, { isLoadingHistory: false }) ?? state);
        }
      } catch {
        set((state) => buildChatBucketPatch(state, gid, { isLoadingHistory: false }) ?? state);
      }
    },

    openChatWindow: async (groupId: string, centerEventId: string) => {
      const gid = String(groupId || "").trim();
      const eid = String(centerEventId || "").trim();
      if (!gid || !eid) return;
      const epoch = beginGroupRequestEpoch(chatWindowRequestEpochByGroup, gid);

      set((state) => buildChatBucketPatch(state, gid, {
        isChatWindowLoading: true,
        chatWindow: {
          groupId: gid,
          centerEventId: eid,
          centerIndex: 0,
          events: [],
          hasMoreBefore: false,
          hasMoreAfter: false,
        },
      }) ?? state);
      try {
        const resp = await api.fetchMessageWindow(gid, eid, { before: 30, after: 30 });
        if (!isLatestGroupRequestEpoch(chatWindowRequestEpochByGroup, gid, epoch)) return;
        if (!resp.ok) {
          set((state) => buildChatBucketPatch(state, gid, { chatWindow: null }) ?? state);
          return;
        }

        const events = (resp.result.events || []).filter((event) => event && event.kind === "chat.message");
        set((state) => buildChatBucketPatch(state, gid, {
          chatWindow: {
            groupId: gid,
            centerEventId: resp.result.center_id,
            centerIndex: resp.result.center_index,
            events,
            hasMoreBefore: !!resp.result.has_more_before,
            hasMoreAfter: !!resp.result.has_more_after,
          },
        }) ?? state);
      } finally {
        if (isLatestGroupRequestEpoch(chatWindowRequestEpochByGroup, gid, epoch)) {
          set((state) => buildChatBucketPatch(state, gid, { isChatWindowLoading: false }) ?? state);
        }
      }
    },

    closeChatWindow: (groupId?: string) =>
      set((state) => {
        const gid = String(groupId || state.selectedGroupId || "").trim();
        beginGroupRequestEpoch(chatWindowRequestEpochByGroup, gid);
        return buildChatBucketPatch(state, gid, {
          chatWindow: null,
          isChatWindowLoading: false,
        }) ?? state;
      }),
  };
}
