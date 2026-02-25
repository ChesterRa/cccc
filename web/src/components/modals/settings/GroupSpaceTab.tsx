import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  GroupSpaceArtifact,
  GroupSpaceJob,
  GroupSpaceProviderAuthStatus,
  GroupSpaceProviderCredentialStatus,
  GroupSpaceRemoteSpace,
  GroupSpaceSource,
  GroupSpaceStatus,
} from "../../../types";
import * as api from "../../../services/api";
import { cardClass, inputClass } from "./types";

interface GroupSpaceTabProps {
  isDark: boolean;
  groupId?: string;
  isActive?: boolean;
}

type QuickSourceType =
  | "web_page"
  | "youtube"
  | "pasted_text"
  | "google_docs"
  | "google_slides"
  | "google_spreadsheet";

type ArtifactKind =
  | "audio"
  | "video"
  | "report"
  | "study_guide"
  | "quiz"
  | "flashcards"
  | "infographic"
  | "slide_deck"
  | "data_table"
  | "mind_map";

const ARTIFACT_KINDS: ArtifactKind[] = [
  "audio",
  "video",
  "report",
  "study_guide",
  "quiz",
  "flashcards",
  "infographic",
  "slide_deck",
  "data_table",
  "mind_map",
];

const ARTIFACT_KIND_LABEL_KEY: Record<ArtifactKind, string> = {
  audio: "groupSpace.artifactKindAudio",
  video: "groupSpace.artifactKindVideo",
  report: "groupSpace.artifactKindReport",
  study_guide: "groupSpace.artifactKindStudyGuide",
  quiz: "groupSpace.artifactKindQuiz",
  flashcards: "groupSpace.artifactKindFlashcards",
  infographic: "groupSpace.artifactKindInfographic",
  slide_deck: "groupSpace.artifactKindSlideDeck",
  data_table: "groupSpace.artifactKindDataTable",
  mind_map: "groupSpace.artifactKindMindMap",
};

function parseJsonObject(raw: string): { ok: true; value: Record<string, unknown> } | { ok: false; error: string } {
  const text = String(raw || "").trim();
  if (!text) return { ok: true, value: {} };
  try {
    const obj = JSON.parse(text);
    if (!obj || typeof obj !== "object" || Array.isArray(obj)) {
      return { ok: false, error: "must be JSON object" };
    }
    return { ok: true, value: obj as Record<string, unknown> };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "invalid JSON" };
  }
}

function normalizeArtifactKind(raw: unknown): ArtifactKind | "" {
  let text = String(raw || "").trim().toLowerCase();
  if (!text) return "";
  if (text.includes(".")) {
    text = text.split(".").pop() || "";
  }
  text = text.replace(/-/g, "_");
  const alias: Record<string, ArtifactKind> = {
    studyguide: "study_guide",
    datatable: "data_table",
    slidedeck: "slide_deck",
    mindmap: "mind_map",
  };
  const normalized = alias[text] || text;
  return (ARTIFACT_KINDS as string[]).includes(normalized) ? (normalized as ArtifactKind) : "";
}

export function GroupSpaceTab({ isDark, groupId, isActive = true }: GroupSpaceTabProps) {
  const { t } = useTranslation("settings");
  const [provider] = useState("notebooklm");
  const [status, setStatus] = useState<GroupSpaceStatus | null>(null);
  const [credential, setCredential] = useState<GroupSpaceProviderCredentialStatus | null>(null);
  const [authFlow, setAuthFlow] = useState<GroupSpaceProviderAuthStatus | null>(null);
  const [spaces, setSpaces] = useState<GroupSpaceRemoteSpace[]>([]);
  const [sources, setSources] = useState<GroupSpaceSource[]>([]);
  const [artifacts, setArtifacts] = useState<GroupSpaceArtifact[]>([]);
  const [authJsonText, setAuthJsonText] = useState("");
  const [jobs, setJobs] = useState<GroupSpaceJob[]>([]);
  const [jobsFilter, setJobsFilter] = useState("");
  const [bindRemoteId, setBindRemoteId] = useState("");
  const [queryText, setQueryText] = useState("");
  const [queryOptionsText, setQueryOptionsText] = useState("{}");
  const [queryAnswer, setQueryAnswer] = useState("");
  const [queryMeta, setQueryMeta] = useState("");
  const [ingestKind, setIngestKind] = useState<"context_sync" | "resource_ingest">("context_sync");
  const [ingestPayloadText, setIngestPayloadText] = useState("{}");
  const [ingestIdempotencyKey, setIngestIdempotencyKey] = useState("");
  const [quickSourceType, setQuickSourceType] = useState<QuickSourceType>("web_page");
  const [quickSourceTitle, setQuickSourceTitle] = useState("");
  const [quickSourceUrl, setQuickSourceUrl] = useState("");
  const [quickSourceText, setQuickSourceText] = useState("");
  const [quickSourceFileId, setQuickSourceFileId] = useState("");
  const [artifactKind, setArtifactKind] = useState<ArtifactKind>("report");
  const [artifactInstructions, setArtifactInstructions] = useState("");
  const [loading, setLoading] = useState(false);
  const [spacesBusy, setSpacesBusy] = useState(false);
  const [sourcesBusy, setSourcesBusy] = useState(false);
  const [artifactsBusy, setArtifactsBusy] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [showStatusDetails, setShowStatusDetails] = useState(false);
  const [showTroubleshooting, setShowTroubleshooting] = useState(false);
  const [showAdvancedOps, setShowAdvancedOps] = useState(false);
  const [hint, setHint] = useState("");
  const [err, setErr] = useState("");
  const autoBindHandledFlowRef = useRef<string>("");
  const connectHintedFlowRef = useRef<string>("");

  const statusTone = useMemo(() => {
    const mode = String(status?.provider?.mode || "disabled");
    if (mode === "active") return isDark ? "text-emerald-300" : "text-emerald-700";
    if (mode === "degraded") return isDark ? "text-amber-300" : "text-amber-700";
    return isDark ? "text-slate-300" : "text-gray-700";
  }, [isDark, status?.provider?.mode]);

  const readinessReason = String(status?.provider?.readiness_reason || "");
  const readinessReasonText = useMemo(() => {
    if (!readinessReason) return "";
    if (readinessReason === "ok") return t("groupSpace.readiness.ok");
    if (readinessReason === "missing_auth") return t("groupSpace.readiness.missingAuth");
    if (readinessReason === "real_disabled_and_stub_disabled") return t("groupSpace.readiness.realDisabled");
    if (readinessReason === "not_ready") return t("groupSpace.readiness.notReady");
    return t("groupSpace.readiness.unknown", { reason: readinessReason });
  }, [readinessReason, t]);

  const realAdapterEnabled = Boolean(status?.provider?.real_adapter_enabled);
  const authConfigured = Boolean(status?.provider?.auth_configured);
  const writeReady = Boolean(status?.provider?.write_ready);
  const connectionState = String(authFlow?.state || "idle");
  const connectionPhase = String(authFlow?.phase || "");
  const connectionMessage = String(authFlow?.message || "");
  const connectionErrorMessage = String(authFlow?.error?.message || "");
  const connectionRunning = connectionState === "running";
  const connected = authConfigured && realAdapterEnabled;
  const boundRemoteSpaceId = String(status?.binding?.remote_space_id || "").trim();
  const boundSpace = spaces.find((item) => String(item.remote_space_id || "").trim() === boundRemoteSpaceId) || null;
  const hasLocalSpaceRoot = Boolean(String(status?.sync?.space_root || "").trim());
  const quickSourceNeedsUrl = quickSourceType === "web_page" || quickSourceType === "youtube";
  const quickSourceNeedsText = quickSourceType === "pasted_text";
  const quickSourceNeedsFileId =
    quickSourceType === "google_docs" ||
    quickSourceType === "google_slides" ||
    quickSourceType === "google_spreadsheet";

  const loadAll = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setLoading(true);
    setErr("");
    try {
      let nextBindingRemoteId = "";
      let nextAuthConfigured = false;
      const statusResp = await api.fetchGroupSpaceStatus(gid, provider);
      if (!statusResp.ok) {
        setErr(statusResp.error?.message || t("groupSpace.loadFailed"));
      } else {
        setStatus(statusResp.result || null);
        const binding = statusResp.result?.binding;
        if (binding?.status === "bound" && String(binding.remote_space_id || "").trim()) {
          nextBindingRemoteId = String(binding.remote_space_id || "").trim();
          setBindRemoteId(nextBindingRemoteId);
        }
        nextAuthConfigured = Boolean(statusResp.result?.provider?.auth_configured);
      }

      if (nextAuthConfigured) {
        setSpacesBusy(true);
        try {
          const spacesResp = await api.fetchGroupSpaceSpaces(gid, provider);
          if (spacesResp.ok) {
            const rawList = Array.isArray(spacesResp.result?.spaces) ? spacesResp.result?.spaces : [];
            const items = rawList
              .filter((item) => Boolean(String(item?.remote_space_id || "").trim()))
              .map((item) => ({
                remote_space_id: String(item?.remote_space_id || "").trim(),
                title: String(item?.title || "").trim(),
                created_at: String(item?.created_at || "").trim(),
                is_owner: Boolean(item?.is_owner),
              }));
            setSpaces(items);
            setBindRemoteId((prev) => {
              const current = String(prev || "").trim();
              if (nextBindingRemoteId) return nextBindingRemoteId;
              if (current && items.some((item) => item.remote_space_id === current)) return current;
              return items.length ? items[0].remote_space_id : "";
            });
          } else {
            setSpaces([]);
          }
        } finally {
          setSpacesBusy(false);
        }
      } else {
        setSpaces([]);
      }

      if (nextAuthConfigured && nextBindingRemoteId) {
        setSourcesBusy(true);
        try {
          const sourcesResp = await api.fetchGroupSpaceSources(gid, provider);
          if (sourcesResp.ok) {
            const rows = Array.isArray(sourcesResp.result?.sources) ? sourcesResp.result.sources : [];
            setSources(rows);
          } else {
            setSources([]);
          }
        } finally {
          setSourcesBusy(false);
        }

        setArtifactsBusy(true);
        try {
          const artifactsResp = await api.fetchGroupSpaceArtifacts(gid, provider);
          if (artifactsResp.ok) {
            const rows = Array.isArray(artifactsResp.result?.artifacts) ? artifactsResp.result.artifacts : [];
            setArtifacts(rows);
          } else {
            setArtifacts([]);
          }
        } finally {
          setArtifactsBusy(false);
        }
      } else {
        setSources([]);
        setArtifacts([]);
      }

      if (showTroubleshooting && showAdvancedOps) {
        const jobsResp = await api.listGroupSpaceJobs({ groupId: gid, provider, state: jobsFilter, limit: 30 });
        if (!jobsResp.ok) {
          const msg = jobsResp.error?.message || t("groupSpace.loadJobsFailed");
          setErr((prev) => prev || msg);
          setJobs([]);
        } else {
          setJobs(Array.isArray(jobsResp.result?.jobs) ? jobsResp.result.jobs : []);
        }
      } else {
        setJobs([]);
      }

      const credentialResp = await api.fetchGroupSpaceProviderCredential(provider);
      if (credentialResp.ok) {
        setCredential(credentialResp.result?.credential || null);
      } else {
        setCredential(null);
      }

      const authResp = await api.controlGroupSpaceProviderAuth({ provider, action: "status" });
      if (authResp.ok) {
        setAuthFlow(authResp.result?.auth || null);
      } else {
        setAuthFlow(null);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("groupSpace.loadFailed"));
    } finally {
      setLoading(false);
    }
  };

  const autoBindAfterConnect = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    try {
      const statusResp = await api.fetchGroupSpaceStatus(gid, provider);
      const binding = statusResp.ok ? statusResp.result?.binding : null;
      const alreadyBound =
        String(binding?.status || "").trim() === "bound" &&
        Boolean(String(binding?.remote_space_id || "").trim());
      if (alreadyBound) return;
      const bindResp = await api.bindGroupSpace(gid, "", provider);
      if (!bindResp.ok) {
        setErr(bindResp.error?.message || t("groupSpace.autoBindFailed"));
        return;
      }
      setStatus(bindResp.result || null);
      const nextRemoteId = String(bindResp.result?.binding?.remote_space_id || "").trim();
      if (nextRemoteId) {
        setBindRemoteId(nextRemoteId);
      }
      setHintWithTimeout(t("groupSpace.autoBindCreated"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.autoBindFailed"));
    }
  };

  useEffect(() => {
    if (!isActive) return;
    if (!groupId) return;
    void loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Only refresh when tab is active/group changes/filter changes.
  }, [isActive, groupId, jobsFilter, showAdvancedOps, showTroubleshooting]);

  useEffect(() => {
    if (!isActive) return;
    if (!groupId) return;
    if (!connectionRunning) return;
    const pollOnce = async () => {
      try {
        const resp = await api.controlGroupSpaceProviderAuth({ provider, action: "status" });
        if (!resp.ok) {
          setErr(resp.error?.message || t("groupSpace.loadFailed"));
          return;
        }
        const next = resp.result?.auth || null;
        setAuthFlow(next);
        const nextState = String(next?.state || "");
        if (nextState && nextState !== "running") {
          await loadAll();
          if (nextState === "succeeded") {
            const flowKey = String(next?.session_id || next?.started_at || "succeeded").trim();
            if (flowKey && connectHintedFlowRef.current !== flowKey) {
              connectHintedFlowRef.current = flowKey;
              setHintWithTimeout(t("groupSpace.googleConnectSuccess"));
            }
            if (flowKey && autoBindHandledFlowRef.current !== flowKey) {
              autoBindHandledFlowRef.current = flowKey;
              await autoBindAfterConnect();
            }
          }
        }
      } catch (e) {
        setErr(e instanceof Error ? e.message : t("groupSpace.loadFailed"));
      }
    };
    void pollOnce();
    const timer = window.setInterval(async () => {
      await pollOnce();
    }, 3000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Poll only while auth flow is running.
  }, [isActive, groupId, connectionRunning]);

  useEffect(() => {
    if (!isActive) return;
    if (!groupId) return;
    if (connectionState !== "succeeded") return;
    const bindingStatus = String(status?.binding?.status || "").trim();
    const bindingRemoteId = String(status?.binding?.remote_space_id || "").trim();
    if (bindingStatus === "bound" && bindingRemoteId) return;
    const flowKey = String(authFlow?.session_id || authFlow?.started_at || "").trim();
    if (!flowKey || autoBindHandledFlowRef.current === flowKey) return;
    autoBindHandledFlowRef.current = flowKey;
    void autoBindAfterConnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Auto-bind once per succeeded auth flow.
  }, [isActive, groupId, connectionState, status?.binding?.status, status?.binding?.remote_space_id, authFlow?.session_id, authFlow?.started_at]);

  useEffect(() => {
    if (showTroubleshooting) return;
    if (!showAdvancedOps) return;
    setShowAdvancedOps(false);
  }, [showTroubleshooting, showAdvancedOps]);

  const setHintWithTimeout = (text: string) => {
    setHint(text);
    window.setTimeout(() => setHint(""), 2400);
  };

  const renderSourceStatus = (raw: unknown): string => {
    const text = String(raw ?? "").trim().toLowerCase();
    if (!text) return t("groupSpace.sourceStatusUnknown");
    if (text === "1" || text === "processing") return t("groupSpace.sourceStatusProcessing");
    if (text === "2" || text === "ready") return t("groupSpace.sourceStatusReady");
    if (text === "3" || text === "error") return t("groupSpace.sourceStatusError");
    if (text === "5" || text === "preparing") return t("groupSpace.sourceStatusPreparing");
    return text;
  };

  const renderArtifactStatus = (raw: unknown): string => {
    const text = String(raw ?? "").trim().toLowerCase();
    if (!text) return t("groupSpace.artifactStatusUnknown");
    if (text === "completed" || text === "succeeded" || text === "ready" || text === "done") {
      return t("groupSpace.artifactStatusCompleted");
    }
    if (text === "running" || text === "processing" || text === "pending") {
      return t("groupSpace.artifactStatusProcessing");
    }
    if (text === "failed" || text === "error") {
      return t("groupSpace.artifactStatusFailed");
    }
    return text;
  };

  const hasStorageCookies = (obj: Record<string, unknown>): boolean => {
    return Array.isArray(obj.cookies) && obj.cookies.length > 0;
  };

  const handleConnectGoogle = async () => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.controlGroupSpaceProviderAuth({
        provider,
        action: "start",
        timeoutSeconds: 900,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.connectStartFailed"));
        return;
      }
      setAuthFlow(resp.result?.auth || null);
      setHintWithTimeout(t("groupSpace.connectStarted"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.connectStartFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleCancelConnect = async () => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.controlGroupSpaceProviderAuth({
        provider,
        action: "cancel",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.connectCancelFailed"));
        return;
      }
      setAuthFlow(resp.result?.auth || null);
      setHintWithTimeout(t("groupSpace.connectCancelRequested"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.connectCancelFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleBind = async (options?: { remoteSpaceId?: string; successKey?: string }) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const remoteSpaceId = String(options?.remoteSpaceId ?? bindRemoteId ?? "").trim();
    const successKey = String(options?.successKey || "groupSpace.bindSuccess");
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.bindGroupSpace(gid, remoteSpaceId, provider);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.bindFailed"));
        return;
      }
      setStatus(resp.result || null);
      const nextRemoteId = String(resp.result?.binding?.remote_space_id || "").trim();
      if (nextRemoteId) {
        setBindRemoteId(nextRemoteId);
      }
      await loadAll();
      setHintWithTimeout(t(successKey));
    } catch {
      setErr(t("groupSpace.bindFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleSyncNow = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.syncGroupSpace({
        groupId: gid,
        provider,
        action: "run",
        force: true,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.syncFailed"));
        return;
      }
      setHintWithTimeout(t("groupSpace.syncDone"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.syncFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleUnbind = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.unbindGroupSpace(gid, provider);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.unbindFailed"));
        return;
      }
      setStatus(resp.result || null);
      setHintWithTimeout(t("groupSpace.unbindSuccess"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.unbindFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleQuickAddSource = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    if (!writeReady) {
      setErr(t("groupSpace.sourceRequireReady"));
      return;
    }
    if (!boundRemoteSpaceId) {
      setErr(t("groupSpace.sourceRequireBind"));
      return;
    }
    const title = String(quickSourceTitle || "").trim();
    const url = String(quickSourceUrl || "").trim();
    const content = String(quickSourceText || "").trim();
    const fileId = String(quickSourceFileId || "").trim();
    if (quickSourceNeedsUrl && !url) {
      setErr(t("groupSpace.sourceUrlRequired"));
      return;
    }
    if (quickSourceNeedsText && !content) {
      setErr(t("groupSpace.sourceTextRequired"));
      return;
    }
    if (quickSourceNeedsFileId && !fileId) {
      setErr(t("groupSpace.sourceFileIdRequired"));
      return;
    }

    const payload: Record<string, unknown> = { source_type: quickSourceType };
    if (title) payload.title = title;
    if (quickSourceNeedsUrl) payload.url = url;
    if (quickSourceNeedsText) payload.content = content;
    if (quickSourceNeedsFileId) payload.file_id = fileId;

    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.ingestGroupSpace({
        groupId: gid,
        provider,
        kind: "resource_ingest",
        payload,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.sourceAddFailed"));
        return;
      }
      setHintWithTimeout(t("groupSpace.sourceAdded", { jobId: resp.result?.job_id || "" }));
      if (quickSourceNeedsUrl) setQuickSourceUrl("");
      if (quickSourceNeedsText) setQuickSourceText("");
      if (quickSourceNeedsFileId) setQuickSourceFileId("");
      await loadAll();
    } catch {
      setErr(t("groupSpace.sourceAddFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleSourceAction = async (action: "refresh" | "delete", sourceId: string) => {
    const gid = String(groupId || "").trim();
    const sid = String(sourceId || "").trim();
    if (!gid || !sid) return;
    if (action === "delete") {
      const row = sources.find((item) => String(item.source_id || "").trim() === sid);
      const label = String(row?.title || "").trim() || sid;
      if (!window.confirm(t("groupSpace.sourceDeleteConfirm", { title: label }))) {
        return;
      }
    }
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.actionGroupSpaceSource({
        groupId: gid,
        provider,
        action,
        sourceId: sid,
      });
      if (!resp.ok) {
        setErr(
          resp.error?.message ||
            (action === "delete" ? t("groupSpace.sourceDeleteFailed") : t("groupSpace.sourceRefreshFailed"))
        );
        return;
      }
      setHintWithTimeout(action === "delete" ? t("groupSpace.sourceDeleted") : t("groupSpace.sourceRefreshed"));
      await loadAll();
    } catch {
      setErr(action === "delete" ? t("groupSpace.sourceDeleteFailed") : t("groupSpace.sourceRefreshFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleGenerateArtifact = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    if (!writeReady) {
      setErr(t("groupSpace.sourceRequireReady"));
      return;
    }
    if (!boundRemoteSpaceId) {
      setErr(t("groupSpace.sourceRequireBind"));
      return;
    }
    const options: Record<string, unknown> = {};
    const instructions = String(artifactInstructions || "").trim();
    if (instructions) {
      options.instructions = instructions;
    }
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.actionGroupSpaceArtifact({
        groupId: gid,
        provider,
        action: "generate",
        kind: artifactKind,
        options,
        wait: true,
        saveToSpace: hasLocalSpaceRoot,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.artifactGenerateFailed"));
        return;
      }
      const outputPath = String(resp.result?.output_path || "").trim();
      if (outputPath) {
        setHintWithTimeout(t("groupSpace.artifactGeneratedSaved", { path: outputPath }));
      } else if (!hasLocalSpaceRoot) {
        setHintWithTimeout(t("groupSpace.artifactGeneratedNoLocalScope", { taskId: String(resp.result?.task_id || "") }));
      } else {
        setHintWithTimeout(t("groupSpace.artifactGenerated", { taskId: String(resp.result?.task_id || "") }));
      }
      await loadAll();
    } catch {
      setErr(t("groupSpace.artifactGenerateFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleArtifactDownload = async (kind: string, artifactId: string) => {
    const gid = String(groupId || "").trim();
    const artifactKindValue = normalizeArtifactKind(kind);
    const artifactIdValue = String(artifactId || "").trim();
    if (!gid || !artifactKindValue) return;
    if (!writeReady) {
      setErr(t("groupSpace.sourceRequireReady"));
      return;
    }
    if (!boundRemoteSpaceId) {
      setErr(t("groupSpace.sourceRequireBind"));
      return;
    }
    if (!hasLocalSpaceRoot) {
      setErr(t("groupSpace.artifactRequireLocalScope"));
      return;
    }
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.actionGroupSpaceArtifact({
        groupId: gid,
        provider,
        action: "download",
        kind: artifactKindValue,
        saveToSpace: true,
        artifactId: artifactIdValue,
        outputFormat:
          artifactKindValue === "quiz" || artifactKindValue === "flashcards" ? "markdown" : "",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.artifactDownloadFailed"));
        return;
      }
      setHintWithTimeout(
        t("groupSpace.artifactDownloaded", { path: String(resp.result?.output_path || "").trim() || "-" })
      );
      await loadAll();
    } catch {
      setErr(t("groupSpace.artifactDownloadFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleIngest = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const parsed = parseJsonObject(ingestPayloadText);
    if (!parsed.ok) {
      setErr(t("groupSpace.invalidJsonObject", { field: t("groupSpace.ingestPayload") }));
      return;
    }
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.ingestGroupSpace({
        groupId: gid,
        provider,
        kind: ingestKind,
        payload: parsed.value,
        idempotencyKey: ingestIdempotencyKey,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.ingestFailed"));
        return;
      }
      setHintWithTimeout(t("groupSpace.ingestAccepted", { jobId: resp.result?.job_id || "" }));
      await loadAll();
    } catch {
      setErr(t("groupSpace.ingestFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleSaveCredential = async () => {
    const parsed = parseJsonObject(authJsonText);
    if (!parsed.ok) {
      setErr(t("groupSpace.invalidJsonObject", { field: t("groupSpace.authJson") }));
      return;
    }
    if (!hasStorageCookies(parsed.value)) {
      setErr(t("groupSpace.storageStateInvalid"));
      return;
    }
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.updateGroupSpaceProviderCredential({
        provider,
        authJson: authJsonText,
        clear: false,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.credentialSaveFailed"));
        return;
      }
      setCredential(resp.result?.credential || null);
      setAuthJsonText("");
      setHintWithTimeout(t("groupSpace.credentialSaved"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.credentialSaveFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleClearCredential = async () => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.updateGroupSpaceProviderCredential({
        provider,
        authJson: "",
        clear: true,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.credentialClearFailed"));
        return;
      }
      setCredential(resp.result?.credential || null);
      setAuthJsonText("");
      setHintWithTimeout(t("groupSpace.credentialCleared"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.credentialClearFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleHealthCheck = async () => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.checkGroupSpaceProviderHealth(provider);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.healthCheckFailed"));
        return;
      }
      if (resp.result?.credential) {
        setCredential(resp.result.credential);
      }
      if (resp.result?.healthy) {
        setHintWithTimeout(t("groupSpace.healthCheckOk"));
      } else {
        const reason = String(resp.result?.error?.message || "");
        setErr(reason || t("groupSpace.healthCheckFailed"));
      }
      await loadAll();
    } catch {
      setErr(t("groupSpace.healthCheckFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleQuery = async () => {
    const gid = String(groupId || "").trim();
    const query = String(queryText || "").trim();
    if (!gid) return;
    if (!query) {
      setErr(t("groupSpace.queryRequired"));
      return;
    }
    const parsed = parseJsonObject(queryOptionsText);
    if (!parsed.ok) {
      setErr(t("groupSpace.invalidJsonObject", { field: t("groupSpace.queryOptions") }));
      return;
    }
    setActionBusy(true);
    setErr("");
    setQueryAnswer("");
    setQueryMeta("");
    try {
      const resp = await api.queryGroupSpace({
        groupId: gid,
        provider,
        query,
        options: parsed.value,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.queryFailed"));
        return;
      }
      const result = resp.result || {
        answer: "",
        degraded: false,
        error: null,
      };
      setQueryAnswer(String(result.answer || ""));
      if (result.degraded) {
        const reason = String(result.error?.message || "");
        setQueryMeta(
          reason ? `${t("groupSpace.degradedResult")}: ${reason}` : t("groupSpace.degradedResult")
        );
      } else {
        setQueryMeta(t("groupSpace.queryOk"));
      }
      await loadAll();
    } catch {
      setErr(t("groupSpace.queryFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleJobAction = async (action: "retry" | "cancel", jobId: string) => {
    const gid = String(groupId || "").trim();
    if (!gid || !jobId) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.actionGroupSpaceJob({
        groupId: gid,
        provider,
        action,
        jobId,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.jobActionFailed"));
        return;
      }
      setHintWithTimeout(action === "retry" ? t("groupSpace.jobRetried") : t("groupSpace.jobCanceled"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.jobActionFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  if (!groupId) {
    return <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.openFromGroup")}</div>;
  }

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("groupSpace.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.description")}</p>
        <div
          className={`mt-2 rounded-lg border px-3 py-2 text-xs ${
            isDark ? "border-amber-700/40 bg-amber-900/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
          }`}
        >
          <div className="font-medium">{t("groupSpace.experimentalTitle")}</div>
          <div className="mt-1">{t("groupSpace.experimentalHint")}</div>
          <div className="mt-1">{t("groupSpace.experimentalDisclaimer")}</div>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.connectionTitle")}</div>
          <div className={`text-xs ${connected ? (isDark ? "text-emerald-300" : "text-emerald-700") : (isDark ? "text-slate-400" : "text-gray-600")}`}>
            {connected ? t("groupSpace.connectionConnected") : t("groupSpace.connectionNotConnected")}
          </div>
        </div>
        <div className={`mt-2 text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
          {t("groupSpace.connectionDescription")}
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          <button
            onClick={() => void handleConnectGoogle()}
            disabled={actionBusy || connectionRunning}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
            } disabled:opacity-50`}
          >
            {connected ? t("groupSpace.reconnectGoogle") : t("groupSpace.connectGoogle")}
          </button>
          <button
            onClick={() => void handleCancelConnect()}
            disabled={actionBusy || !connectionRunning}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-rose-900/40 hover:bg-rose-800/40 text-rose-300 border border-rose-700/60" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"
            } disabled:opacity-50`}
          >
            {t("groupSpace.cancelConnect")}
          </button>
          <button
            onClick={() => void loadAll()}
            disabled={actionBusy || loading}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {t("groupSpace.refresh")}
          </button>
        </div>
        <div className="mt-3 text-xs space-y-1">
          <div className={isDark ? "text-slate-400" : "text-gray-600"}>
            {t("groupSpace.connectionState")}: {connectionState}
            {connectionPhase ? ` · ${connectionPhase}` : ""}
          </div>
          {connectionMessage ? (
            <div className={isDark ? "text-slate-300" : "text-gray-700"}>{connectionMessage}</div>
          ) : null}
          {connectionErrorMessage ? (
            <div className={isDark ? "text-amber-300" : "text-amber-700"}>{connectionErrorMessage}</div>
          ) : null}
          {!connected ? (
            <div className={isDark ? "text-slate-500" : "text-gray-500"}>
              {t("groupSpace.connectionHint")}
            </div>
          ) : null}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.statusTitle")}</div>
            <span
              title={t("groupSpace.statusInfoTooltip")}
              aria-label={t("groupSpace.statusInfoLabel")}
              className={`inline-flex h-4 w-4 items-center justify-center rounded-full text-[10px] font-semibold cursor-help ${
                isDark ? "border border-slate-600 text-slate-300" : "border border-gray-300 text-gray-600"
              }`}
            >
              !
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setShowStatusDetails((prev) => !prev)}
              disabled={actionBusy || loading}
              className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {showStatusDetails ? t("groupSpace.hideStatusDetails") : t("groupSpace.showStatusDetails")}
            </button>
            <button
              onClick={() => void loadAll()}
              disabled={actionBusy || loading}
              className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {loading ? t("common:loading") : t("groupSpace.refresh")}
            </button>
          </div>
        </div>
        <div className="mt-2 text-xs space-y-1">
          <div className={`${statusTone}`}>
            {t("groupSpace.providerMode")}: {String(status?.provider?.mode || "disabled")}
          </div>
          <div className={isDark ? "text-slate-400" : "text-gray-600"}>
            {t("groupSpace.providerWriteReady")}: {writeReady ? t("common:yes") : t("common:no")}
            {!writeReady && readinessReasonText ? ` (${readinessReasonText})` : ""}
          </div>
          <div className={isDark ? "text-slate-400" : "text-gray-600"}>
            {t("groupSpace.bindingStatus")}: {String(status?.binding?.status || "unbound")}
            {boundRemoteSpaceId ? ` · ${boundSpace?.title ? `${boundSpace.title} (${boundRemoteSpaceId})` : boundRemoteSpaceId}` : ""}
          </div>
          <div className={isDark ? "text-slate-400" : "text-gray-600"}>
            {t("groupSpace.syncLastRunAt")}: {status?.sync?.last_run_at ? String(status.sync.last_run_at) : t("groupSpace.syncNotRunYet")}
          </div>
          {status?.provider?.last_error ? (
            <div className={isDark ? "text-amber-300" : "text-amber-700"}>
              {t("groupSpace.lastError")}: {String(status.provider.last_error)}
            </div>
          ) : null}
          {status?.sync?.last_error ? (
            <div className={isDark ? "text-amber-300" : "text-amber-700"}>
              {t("groupSpace.syncLastError")}: {String(status.sync.last_error)}
            </div>
          ) : null}
        </div>
        {showStatusDetails ? (
          <div className={`mt-3 pt-3 border-t text-xs space-y-1 ${isDark ? "border-slate-700" : "border-gray-200"}`}>
            <div className={isDark ? "text-slate-400" : "text-gray-600"}>
              {t("groupSpace.realAdapter")}: {status?.provider?.real_adapter_enabled ? t("common:yes") : t("common:no")}
              {" · "}
              {t("groupSpace.stubAdapter")}: {status?.provider?.stub_adapter_enabled ? t("common:yes") : t("common:no")}
              {" · "}
              {t("groupSpace.authConfigured")}: {status?.provider?.auth_configured ? t("common:yes") : t("common:no")}
            </div>
            <div className={isDark ? "text-slate-400" : "text-gray-600"}>
              {t("groupSpace.syncConverged")}: {status?.sync?.converged ? t("common:yes") : t("common:no")}
              {" · "}
              {t("groupSpace.syncUnsynced")}: {Number(status?.sync?.unsynced_count || 0)}
            </div>
            <div className={isDark ? "text-slate-400" : "text-gray-600"}>
              {t("groupSpace.queueSummary")}: P {Number(status?.queue_summary?.pending || 0)} / R {Number(status?.queue_summary?.running || 0)} / F {Number(status?.queue_summary?.failed || 0)}
            </div>
            {status?.sync?.space_root ? (
              <div className={isDark ? "text-slate-500" : "text-gray-500"}>
                {t("groupSpace.spaceRoot")}: <span className="font-mono break-all">{String(status.sync.space_root)}</span>
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.bindingTitle")}</div>
        <div className="mt-2 space-y-3">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.selectNotebook")}</label>
            <select
              value={bindRemoteId}
              onChange={(e) => setBindRemoteId(e.target.value)}
              disabled={actionBusy || spacesBusy || !writeReady || !spaces.length}
              className={inputClass(isDark)}
            >
              {!spaces.length ? <option value="">{t("groupSpace.noNotebookOptions")}</option> : null}
              {bindRemoteId && !spaces.some((item) => String(item.remote_space_id || "").trim() === String(bindRemoteId || "").trim()) ? (
                <option value={bindRemoteId}>{t("groupSpace.currentNotebookFallback", { id: bindRemoteId })}</option>
              ) : null}
              {spaces.map((item) => {
                const rid = String(item.remote_space_id || "").trim();
                const title = String(item.title || "").trim() || rid;
                const ownerTag = item.is_owner ? "" : ` · ${t("groupSpace.sharedTag")}`;
                return (
                  <option key={rid} value={rid}>
                    {`${title}${ownerTag}`}
                  </option>
                );
              })}
            </select>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t("groupSpace.autoBindHint")}
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              onClick={() => void handleBind({ remoteSpaceId: bindRemoteId, successKey: "groupSpace.bindSuccess" })}
              disabled={actionBusy || spacesBusy || !writeReady || !String(bindRemoteId || "").trim()}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] min-w-[132px] font-medium transition-colors ${
                isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.useSelectedNotebook")}
            </button>
            <button
              onClick={() => void handleBind({ remoteSpaceId: "", successKey: "groupSpace.autoBindCreated" })}
              disabled={actionBusy || !writeReady}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] min-w-[132px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.createAndBind")}
            </button>
            <button
              onClick={() => void handleUnbind()}
              disabled={actionBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] min-w-[108px] font-medium transition-colors ${
                isDark ? "bg-rose-900/40 hover:bg-rose-800/40 text-rose-300 border border-rose-700/60" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.unbind")}
            </button>
            <button
              onClick={() => void handleSyncNow()}
              disabled={actionBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] min-w-[108px] font-medium transition-colors ${
                isDark ? "bg-indigo-900/40 hover:bg-indigo-800/40 text-indigo-300 border border-indigo-700/60" : "bg-indigo-50 hover:bg-indigo-100 text-indigo-700 border border-indigo-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.syncNow")}
            </button>
          </div>
          {!writeReady ? (
            <div className={`text-xs ${isDark ? "text-amber-300" : "text-amber-700"}`}>
              {t("groupSpace.completeGoogleConnectFirst")}
            </div>
          ) : null}
        </div>
      </div>

      {showTroubleshooting && showAdvancedOps ? (
      <>
      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.addSourceTitle")}</div>
        <div className={`mt-1 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.addSourceHint")}</div>
        <div className={`mt-1 text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.addSourceStorageHint")}</div>
        <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.sourceType")}</label>
            <select
              value={quickSourceType}
              onChange={(e) => setQuickSourceType((e.target.value as QuickSourceType) || "web_page")}
              disabled={actionBusy}
              className={inputClass(isDark)}
            >
              <option value="web_page">{t("groupSpace.sourceTypeWebPage")}</option>
              <option value="youtube">{t("groupSpace.sourceTypeYouTube")}</option>
              <option value="pasted_text">{t("groupSpace.sourceTypePastedText")}</option>
              <option value="google_docs">{t("groupSpace.sourceTypeGoogleDocs")}</option>
              <option value="google_slides">{t("groupSpace.sourceTypeGoogleSlides")}</option>
              <option value="google_spreadsheet">{t("groupSpace.sourceTypeGoogleSheets")}</option>
            </select>
          </div>
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.sourceTitle")}</label>
            <input
              value={quickSourceTitle}
              onChange={(e) => setQuickSourceTitle(e.target.value)}
              placeholder={t("groupSpace.sourceTitlePlaceholder")}
              disabled={actionBusy}
              className={inputClass(isDark)}
            />
          </div>

          {quickSourceNeedsUrl ? (
            <div className="sm:col-span-2">
              <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.sourceUrl")}</label>
              <input
                value={quickSourceUrl}
                onChange={(e) => setQuickSourceUrl(e.target.value)}
                placeholder={t("groupSpace.sourceUrlPlaceholder")}
                disabled={actionBusy}
                className={inputClass(isDark)}
              />
            </div>
          ) : null}

          {quickSourceNeedsText ? (
            <div className="sm:col-span-2">
              <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.sourceText")}</label>
              <textarea
                value={quickSourceText}
                onChange={(e) => setQuickSourceText(e.target.value)}
                rows={3}
                placeholder={t("groupSpace.sourceTextPlaceholder")}
                disabled={actionBusy}
                className={`${inputClass(isDark)} resize-y`}
              />
            </div>
          ) : null}

          {quickSourceNeedsFileId ? (
            <div className="sm:col-span-2">
              <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.sourceFileId")}</label>
              <input
                value={quickSourceFileId}
                onChange={(e) => setQuickSourceFileId(e.target.value)}
                placeholder={t("groupSpace.sourceFileIdPlaceholder")}
                disabled={actionBusy}
                className={inputClass(isDark)}
              />
            </div>
          ) : null}

          <div className="sm:col-span-2">
            <button
              onClick={() => void handleQuickAddSource()}
              disabled={actionBusy || !writeReady || !boundRemoteSpaceId}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.addSource")}
            </button>
          </div>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.sourcesTitle")}</div>
            <div className={`mt-1 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.sourcesHint")}</div>
          </div>
          <button
            onClick={() => void loadAll()}
            disabled={actionBusy || loading || sourcesBusy}
            className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {t("groupSpace.refreshSources")}
          </button>
        </div>
        <div className="mt-3 space-y-2">
          {sourcesBusy ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("common:loading")}</div>
          ) : !sources.length ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.noSources")}</div>
          ) : (
            sources.map((source) => {
              const sid = String(source.source_id || "").trim();
              const title = String(source.title || "").trim() || sid;
              const kind = String(source.kind || "").trim() || "-";
              const statusText = renderSourceStatus(source.status);
              const url = String(source.url || "").trim();
              return (
                <div key={sid} className={`rounded border p-2 text-xs ${isDark ? "border-slate-700 bg-slate-900/50 text-slate-200" : "border-gray-200 bg-white text-gray-800"}`}>
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-medium break-all">{title}</div>
                    <div className={isDark ? "text-slate-400" : "text-gray-600"}>
                      {kind} · {statusText}
                    </div>
                  </div>
                  <div className={`mt-1 font-mono break-all ${isDark ? "text-slate-500" : "text-gray-500"}`}>{sid}</div>
                  {url ? (
                    <div className={`mt-1 break-all ${isDark ? "text-slate-400" : "text-gray-600"}`}>{url}</div>
                  ) : null}
                  <div className="mt-2 flex gap-2">
                    <button
                      onClick={() => void handleSourceAction("refresh", sid)}
                      disabled={actionBusy}
                      className={`px-2 py-1 rounded border ${
                        isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-800 hover:bg-gray-50"
                      } disabled:opacity-40`}
                    >
                      {t("groupSpace.sourceRefresh")}
                    </button>
                    <button
                      onClick={() => void handleSourceAction("delete", sid)}
                      disabled={actionBusy}
                      className={`px-2 py-1 rounded border ${
                        isDark ? "border-rose-700/70 text-rose-300 hover:bg-rose-900/30" : "border-rose-300 text-rose-700 hover:bg-rose-50"
                      } disabled:opacity-40`}
                    >
                      {t("groupSpace.sourceDelete")}
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.artifactsTitle")}</div>
            <div className={`mt-1 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.artifactsHint")}</div>
          </div>
          <button
            onClick={() => void loadAll()}
            disabled={actionBusy || loading || artifactsBusy}
            className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {t("groupSpace.refreshArtifacts")}
          </button>
        </div>
        <div className="mt-3 grid grid-cols-1 sm:grid-cols-3 gap-2">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.artifactKind")}</label>
            <select
              value={artifactKind}
              onChange={(e) => setArtifactKind((e.target.value as ArtifactKind) || "report")}
              disabled={actionBusy}
              className={inputClass(isDark)}
            >
              {ARTIFACT_KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {t(ARTIFACT_KIND_LABEL_KEY[kind])}
                </option>
              ))}
            </select>
          </div>
          <div className="sm:col-span-2">
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.artifactInstructions")}</label>
            <input
              value={artifactInstructions}
              onChange={(e) => setArtifactInstructions(e.target.value)}
              placeholder={t("groupSpace.artifactInstructionsPlaceholder")}
              disabled={actionBusy}
              className={inputClass(isDark)}
            />
          </div>
          <div className="sm:col-span-3">
            <button
              onClick={() => void handleGenerateArtifact()}
              disabled={actionBusy || !writeReady || !boundRemoteSpaceId}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.artifactGenerate")}
            </button>
          </div>
        </div>

        <div className="mt-3 space-y-2">
          {artifactsBusy ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("common:loading")}</div>
          ) : !artifacts.length ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.noArtifacts")}</div>
          ) : (
            artifacts.map((artifact) => {
              const aid = String(artifact.artifact_id || "").trim();
              const kind = normalizeArtifactKind(artifact.kind);
              const kindLabel = kind || String(artifact.kind || "").trim() || "-";
              const statusText = renderArtifactStatus(artifact.status);
              const title = String(artifact.title || "").trim() || aid || "-";
              const createdAt = String(artifact.created_at || "").trim();
              const canDownload = Boolean(kind);
              return (
                <div
                  key={aid ? `${kind}:${aid}` : `${kind}:${createdAt}:${title}`}
                  className={`rounded border p-2 text-xs ${isDark ? "border-slate-700 bg-slate-900/50 text-slate-200" : "border-gray-200 bg-white text-gray-800"}`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-medium break-all">{title}</div>
                    <div className={isDark ? "text-slate-400" : "text-gray-600"}>
                      {kindLabel} · {statusText}
                    </div>
                  </div>
                  {aid ? <div className={`mt-1 font-mono break-all ${isDark ? "text-slate-500" : "text-gray-500"}`}>{aid}</div> : null}
                  {createdAt ? <div className={`mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{createdAt}</div> : null}
                  {artifact.url ? (
                    <div className={`mt-1 break-all ${isDark ? "text-slate-400" : "text-gray-600"}`}>{String(artifact.url)}</div>
                  ) : null}
                  <div className="mt-2 flex gap-2">
                    <button
                      onClick={() => void handleArtifactDownload(kind, aid)}
                      disabled={!canDownload || actionBusy || !hasLocalSpaceRoot}
                      className={`px-2 py-1 rounded border ${
                        isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-800 hover:bg-gray-50"
                      } disabled:opacity-40`}
                    >
                      {t("groupSpace.artifactDownload")}
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
        {!hasLocalSpaceRoot ? (
          <div className={`mt-2 text-xs ${isDark ? "text-amber-300" : "text-amber-700"}`}>
            {t("groupSpace.artifactLocalScopeHint")}
          </div>
        ) : null}
      </div>
      </>
      ) : null}

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.troubleshootingTitle")}</div>
            <div className={`mt-1 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.troubleshootingHint")}</div>
          </div>
          <button
            onClick={() => setShowTroubleshooting((prev) => !prev)}
            disabled={actionBusy}
            className={`px-3 py-2 rounded-lg text-sm min-h-[40px] font-medium transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {showTroubleshooting ? t("groupSpace.hideTroubleshooting") : t("groupSpace.showTroubleshooting")}
          </button>
        </div>
        {showTroubleshooting ? (
          <div className="mt-3 space-y-3">
            <details>
              <summary className={`cursor-pointer text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>
                {t("groupSpace.advancedCredentialTitle")}
              </summary>
              <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.advancedCredentialHint")}</div>
              <div className={`mt-2 text-xs space-y-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                <div>{t("groupSpace.credentialConfigured")}: {credential?.configured ? t("common:yes") : t("common:no")}</div>
                <div>{t("groupSpace.credentialSource")}: {String(credential?.source || "none")}</div>
                {credential?.masked_value ? (
                  <div>{t("groupSpace.credentialMaskedValue")}: <span className="font-mono">{credential.masked_value}</span></div>
                ) : null}
                {credential?.updated_at ? (
                  <div>{t("groupSpace.credentialUpdatedAt")}: {String(credential.updated_at)}</div>
                ) : null}
              </div>
              <div className="mt-2 space-y-2">
                <div>
                  <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.authJson")}</label>
                  <textarea
                    value={authJsonText}
                    onChange={(e) => setAuthJsonText(e.target.value)}
                    rows={3}
                    placeholder={t("groupSpace.authJsonPlaceholder")}
                    className={`${inputClass(isDark)} resize-y`}
                  />
                </div>
                <div className="flex flex-wrap gap-2">
                  <button
                    onClick={() => void handleSaveCredential()}
                    disabled={actionBusy}
                    className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                      isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                    } disabled:opacity-50`}
                  >
                    {t("groupSpace.saveCredential")}
                  </button>
                  <button
                    onClick={() => void handleClearCredential()}
                    disabled={actionBusy}
                    className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                      isDark ? "bg-rose-900/40 hover:bg-rose-800/40 text-rose-300 border border-rose-700/60" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"
                    } disabled:opacity-50`}
                  >
                    {t("groupSpace.clearCredential")}
                  </button>
                  <button
                    onClick={() => void handleHealthCheck()}
                    disabled={actionBusy}
                    className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                      isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
                    } disabled:opacity-50`}
                  >
                    {t("groupSpace.healthCheck")}
                  </button>
                </div>
              </div>
            </details>

            <div className={`rounded-lg border p-3 ${isDark ? "border-slate-700" : "border-gray-200"}`}>
              <div className="flex items-center justify-between gap-2">
                <div>
                  <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.advancedOpsTitle")}</div>
                  <div className={`mt-1 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.advancedOpsHint")}</div>
                </div>
                <button
                  onClick={() => setShowAdvancedOps((prev) => !prev)}
                  disabled={actionBusy}
                  className={`px-3 py-2 rounded-lg text-sm min-h-[40px] font-medium transition-colors ${
                    isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                  } disabled:opacity-50`}
                >
                  {showAdvancedOps ? t("groupSpace.hideAdvancedOps") : t("groupSpace.showAdvancedOps")}
                </button>
              </div>
            </div>
          </div>
        ) : null}
      </div>

      {showTroubleshooting && showAdvancedOps ? (
      <>
      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.queryTitle")}</div>
        <div className="mt-2 space-y-2">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.query")}</label>
            <input value={queryText} onChange={(e) => setQueryText(e.target.value)} placeholder={t("groupSpace.queryPlaceholder")} className={inputClass(isDark)} />
          </div>
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.queryOptions")}</label>
            <textarea value={queryOptionsText} onChange={(e) => setQueryOptionsText(e.target.value)} rows={2} className={`${inputClass(isDark)} resize-y`} />
          </div>
          <button
            onClick={() => void handleQuery()}
            disabled={actionBusy}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {t("groupSpace.runQuery")}
          </button>
          {queryMeta ? <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>{queryMeta}</div> : null}
          {queryAnswer ? (
            <pre className={`text-xs p-2 rounded border whitespace-pre-wrap ${isDark ? "bg-slate-900 border-slate-700 text-slate-200" : "bg-white border-gray-200 text-gray-800"}`}>
              {queryAnswer}
            </pre>
          ) : null}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.ingestTitle")}</div>
        <div className="mt-2 grid grid-cols-1 sm:grid-cols-3 gap-2">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.ingestKind")}</label>
            <select
              value={ingestKind}
              onChange={(e) => setIngestKind((e.target.value as "context_sync" | "resource_ingest") || "context_sync")}
              className={inputClass(isDark)}
            >
              <option value="context_sync">context_sync</option>
              <option value="resource_ingest">resource_ingest</option>
            </select>
          </div>
          <div className="sm:col-span-2">
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.idempotencyKey")}</label>
            <input
              value={ingestIdempotencyKey}
              onChange={(e) => setIngestIdempotencyKey(e.target.value)}
              placeholder={t("groupSpace.idempotencyPlaceholder")}
              className={inputClass(isDark)}
            />
          </div>
          <div className="sm:col-span-3">
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("groupSpace.ingestPayload")}</label>
            <textarea value={ingestPayloadText} onChange={(e) => setIngestPayloadText(e.target.value)} rows={3} className={`${inputClass(isDark)} resize-y`} />
          </div>
          <div className="sm:col-span-3">
            <button
              onClick={() => void handleIngest()}
              disabled={actionBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {t("groupSpace.submitIngest")}
            </button>
          </div>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("groupSpace.jobsTitle")}</div>
          <select value={jobsFilter} onChange={(e) => setJobsFilter(e.target.value)} className={`${inputClass(isDark)} max-w-[180px]`}>
            <option value="">{t("groupSpace.filterAll")}</option>
            <option value="pending">pending</option>
            <option value="running">running</option>
            <option value="failed">failed</option>
            <option value="succeeded">succeeded</option>
            <option value="canceled">canceled</option>
          </select>
        </div>
        <div className="mt-2 space-y-2">
          {!jobs.length ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("groupSpace.noJobs")}</div>
          ) : (
            jobs.map((job) => {
              const canRetry = job.state === "failed" || job.state === "canceled";
              const canCancel = job.state === "pending" || job.state === "running";
              return (
                <div key={job.job_id} className={`rounded border p-2 text-xs ${isDark ? "border-slate-700 bg-slate-900/50 text-slate-200" : "border-gray-200 bg-white text-gray-800"}`}>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="font-mono break-all">{job.job_id}</div>
                    <div>{job.state}</div>
                  </div>
                  <div className={`mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                    {job.kind} · {t("groupSpace.attemptLabel")}: {job.attempt}/{job.max_attempts}
                  </div>
                  {job.last_error?.message ? (
                    <div className={`mt-1 ${isDark ? "text-amber-300" : "text-amber-700"}`}>{job.last_error.message}</div>
                  ) : null}
                  <div className="mt-2 flex gap-2">
                    <button
                      onClick={() => void handleJobAction("retry", job.job_id)}
                      disabled={!canRetry || actionBusy}
                      className={`px-2 py-1 rounded border ${
                        isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-800 hover:bg-gray-50"
                      } disabled:opacity-40`}
                    >
                      {t("groupSpace.retry")}
                    </button>
                    <button
                      onClick={() => void handleJobAction("cancel", job.job_id)}
                      disabled={!canCancel || actionBusy}
                      className={`px-2 py-1 rounded border ${
                        isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-800 hover:bg-gray-50"
                      } disabled:opacity-40`}
                    >
                      {t("groupSpace.cancel")}
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>
      </>
      ) : null}

      {err ? <div className={`text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{err}</div> : null}
      {hint ? <div className={`text-xs ${isDark ? "text-emerald-300" : "text-emerald-600"}`}>{hint}</div> : null}
    </div>
  );
}
