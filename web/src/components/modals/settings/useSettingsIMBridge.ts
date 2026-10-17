import { useCallback, useEffect, useRef, useState } from "react";
import type { TFunction } from "i18next";
import type { IMPlatform, IMStatus, WeixinLoginStatus } from "../../../types";
import * as api from "../../../services/api";
import {
  canStartIMBridge,
  type IMConfigDraft,
  saveAndStartIMBridge,
  saveIMConfigDraft,
} from "./imBridgeConfig";
import { shouldPollWeixinLogin } from "./weixinLoginPolling";
import type { GroupTabId, SettingsScope } from "./types";

export function useSettingsIMBridge(args: {
  isOpen: boolean;
  groupId?: string;
  scope: SettingsScope;
  groupTab: GroupTabId;
  hasSettingsTarget: boolean;
  t: TFunction<"settings">;
}) {
  const { isOpen, groupId, scope, groupTab, hasSettingsTarget, t } = args;

  const [imStatus, setImStatus] = useState<IMStatus | null>(null);
  const [imPlatform, setImPlatform] = useState<IMPlatform>("telegram");
  const [imBotTokenEnv, setImBotTokenEnvState] = useState("");
  const [imAppTokenEnv, setImAppTokenEnv] = useState("");
  const [imMattermostUrl, setImMattermostUrlState] = useState("");
  // Feishu fields
  const [imFeishuDomain, setImFeishuDomain] = useState("https://open.feishu.cn");
  const [imFeishuAppId, setImFeishuAppId] = useState("");
  const [imFeishuAppSecret, setImFeishuAppSecret] = useState("");
  // DingTalk fields
  const [imDingtalkAppKey, setImDingtalkAppKey] = useState("");
  const [imDingtalkAppSecret, setImDingtalkAppSecret] = useState("");
  const [imDingtalkRobotCode, setImDingtalkRobotCode] = useState("");
  // WeCom fields
  const [imWecomBotId, setImWecomBotId] = useState("");
  const [imWecomSecret, setImWecomSecret] = useState("");
  // Weixin fields
  const [imWeixinAccountId, setImWeixinAccountId] = useState("");
  const [weixinLoginStatus, setWeixinLoginStatus] = useState<WeixinLoginStatus | null>(null);
  const [imBusy, setImBusy] = useState(false);
  const [imConfigError, setImConfigError] = useState<{ groupId: string; message: string } | null>(
    null,
  );
  const imLoadSeq = useRef(0);
  const imPlatformSelectionSeq = useRef(0);
  const imMattermostEditSeq = useRef(0);
  const imMattermostVisitSeq = useRef(0);
  const imActionScope = useRef({ groupId, isOpen, platform: imPlatform });
  const imCurrentPlatform = useRef(imPlatform);
  const imMattermostBusy = useRef(false);
  if (
    imActionScope.current.groupId !== groupId ||
    imActionScope.current.isOpen !== isOpen ||
    imActionScope.current.platform !== imPlatform
  ) {
    if (imActionScope.current.platform === "mattermost" || imPlatform === "mattermost") {
      imMattermostVisitSeq.current += 1;
    }
    imActionScope.current = { groupId, isOpen, platform: imPlatform };
  }
  imCurrentPlatform.current = imPlatform;
  useEffect(
    () => () => {
      // AppModals unmounts settings on close; stale management continuations must not reload or start.
      imActionScope.current = { ...imActionScope.current };
    },
    [],
  );
  // Only a login explicitly started in this modal may continue into bridge startup.
  const weixinLoginOwner = useRef<{ scope: typeof imActionScope.current } | null>(null);
  const imInitiallyLoaded = useRef(false);
  const imTabActive = !hasSettingsTarget && scope === "group" && groupTab === "im";

  // Weixin login and Mattermost mutations belong to the initiating Group and platform visit.
  const currentIMAction = useCallback(() => {
    const actionScope = imActionScope.current;
    const edit = imMattermostEditSeq.current;
    const visit = imMattermostVisitSeq.current;
    if (imPlatform === "mattermost") imMattermostBusy.current = true;
    const isCurrent = () =>
      (imPlatform !== "mattermost" &&
        imPlatform !== "weixin" &&
        imCurrentPlatform.current !== "mattermost" &&
        imMattermostVisitSeq.current === visit) ||
      imActionScope.current === actionScope;
    return {
      isCurrent,
      canReload: () =>
        isCurrent() && (imPlatform !== "mattermost" || imMattermostEditSeq.current === edit),
    };
  }, [imPlatform]);

  // IM config drafts cache (per-platform local edits, not yet saved to server)
  const [imConfigDrafts, setImConfigDrafts] = useState<Partial<Record<IMPlatform, IMConfigDraft>>>(
    {},
  );

  useEffect(() => {
    setImConfigDrafts((drafts) => {
      if (!drafts.mattermost) return drafts;
      const next = { ...drafts };
      delete next.mattermost;
      return next;
    });
  }, [groupId]);

  useEffect(() => {
    if (imMattermostBusy.current || imPlatform === "mattermost") {
      imMattermostBusy.current = false;
      setImBusy(false);
    }
  }, [isOpen, groupId, imPlatform]);

  const clearIMCredentialsDraft = () => {
    setImBotTokenEnvState("");
    setImAppTokenEnv("");
    setImMattermostUrlState("");
    setImFeishuDomain("https://open.feishu.cn");
    setImFeishuAppId("");
    setImFeishuAppSecret("");
    setImDingtalkAppKey("");
    setImDingtalkAppSecret("");
    setImDingtalkRobotCode("");
    setImWecomBotId("");
    setImWecomSecret("");
    setImWeixinAccountId("");
  };

  const resetIMState = () => {
    setImConfigError(null);
    setImStatus(null);
    setWeixinLoginStatus(null);
    setImPlatform("telegram");
    clearIMCredentialsDraft();
  };

  const loadIMStatus = useCallback(
    async (opts?: { isCurrent?: () => boolean; canReloadConfig?: () => boolean }) => {
      const gid = String(groupId || "").trim();
      const seq = ++imLoadSeq.current;
      const selection = imPlatformSelectionSeq.current;
      const edit = imMattermostEditSeq.current;
      const isCurrent = (platform?: unknown) =>
        seq === imLoadSeq.current &&
        opts?.isCurrent?.() !== false &&
        (platform !== "mattermost" || selection === imPlatformSelectionSeq.current);
      // Editing a draft blocks configuration hydration, not authoritative runtime status.
      const canReloadConfig = () =>
        opts?.canReloadConfig?.() !== false && imMattermostEditSeq.current === edit;
      if (!gid) return;
      try {
        const statusResp = await api.fetchIMStatus(gid);
        if (!isCurrent(statusResp.ok ? statusResp.result.platform : undefined)) return;
        if (statusResp.ok) {
          setImStatus(statusResp.result);
          if (canReloadConfig() && statusResp.result.platform) {
            setImPlatform(statusResp.result.platform as IMPlatform);
          }
        }
        if (!canReloadConfig()) return;
        const configResp = await api.fetchIMConfig(gid);
        if (
          !isCurrent(configResp.ok ? configResp.result.im?.platform : undefined) ||
          !canReloadConfig()
        )
          return;
        if (configResp.ok && configResp.result.im) {
          const im = configResp.result.im;
          if (im.platform) setImPlatform(im.platform);
          setImBotTokenEnvState(im.bot_token_env || im.bot_token || im.token_env || im.token || "");
          setImAppTokenEnv(im.app_token_env || im.app_token || "");
          setImMattermostUrlState(im.mattermost_url || "");
          {
            const raw = String(im.feishu_domain || "https://open.feishu.cn").trim();
            const canon = raw
              .replace(/\/+$/, "")
              .replace(/\/open-apis$/, "")
              .replace(/^open\.larksuite\.com$/i, "https://open.larkoffice.com")
              .replace(/^https?:\/\/open\.larksuite\.com$/i, "https://open.larkoffice.com")
              .replace(/^open\.larkoffice\.com$/i, "https://open.larkoffice.com");
            setImFeishuDomain(canon);
          }
          setImFeishuAppId(im.feishu_app_id || im.feishu_app_id_env || "");
          setImFeishuAppSecret(im.feishu_app_secret || im.feishu_app_secret_env || "");
          setImDingtalkAppKey(im.dingtalk_app_key || im.dingtalk_app_key_env || "");
          setImDingtalkAppSecret(im.dingtalk_app_secret || im.dingtalk_app_secret_env || "");
          setImDingtalkRobotCode(im.dingtalk_robot_code || im.dingtalk_robot_code_env || "");
          setImWecomBotId(im.wecom_bot_id || "");
          setImWecomSecret(im.wecom_secret || "");
          setImWeixinAccountId(im.weixin_account_id || "");
        }
      } catch (e) {
        console.error("Failed to load IM status:", e);
      }
    },
    [groupId],
  );

  useEffect(() => {
    imInitiallyLoaded.current = false;
    weixinLoginOwner.current = null;
    setImBusy(false);
    resetIMState();
    return () => {
      imLoadSeq.current += 1;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Reset once per settings opening/Group, not when revisiting a draft.
  }, [isOpen, groupId]);

  useEffect(() => {
    if (!isOpen || !imTabActive || imInitiallyLoaded.current) return;
    imInitiallyLoaded.current = true;
    void loadIMStatus();
  }, [isOpen, groupId, imTabActive, loadIMStatus]);

  const toWeixinErrorStatus = useCallback(
    (message: string): WeixinLoginStatus => ({
      status: "error",
      logged_in: false,
      account_id: "",
      qrcode_url: "",
      qr_ascii: "",
      error: String(message || "").trim(),
      running: false,
      pid: null,
      updated_at: new Date().toISOString(),
    }),
    [],
  );

  useEffect(() => {
    if (!isOpen || !groupId || imPlatform !== "weixin" || imBusy || imStatus?.group_id !== groupId)
      return;
    // A scan already in progress keeps completing across settings tabs.
    const needsPoll = shouldPollWeixinLogin({
      running: weixinLoginStatus?.running ?? false,
      status: weixinLoginStatus?.status ?? "",
    });
    if (!imTabActive && !needsPoll) return;
    let cancelled = false;
    let loading = false;
    const loadWeixinStatus = async () => {
      if (loading) return;
      loading = true;
      try {
        const resp = await api.fetchWeixinLoginStatus(groupId);
        if (cancelled) return;
        if (resp.ok) {
          setWeixinLoginStatus(resp.result ?? null);
        } else {
          setWeixinLoginStatus(
            toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinStatusLoadFailed")),
          );
        }
      } catch {
        if (!cancelled) {
          setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinStatusLoadFailed")));
        }
      } finally {
        loading = false;
      }
    };
    void loadWeixinStatus();
    if (!needsPoll)
      return () => {
        cancelled = true;
      };
    const timer = window.setInterval(() => {
      void loadWeixinStatus();
    }, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [
    isOpen,
    groupId,
    imPlatform,
    imBusy,
    imStatus?.group_id,
    imTabActive,
    weixinLoginStatus?.running,
    weixinLoginStatus?.status,
    t,
    toWeixinErrorStatus,
  ]);

  useEffect(() => {
    if (
      !isOpen ||
      imPlatform !== "weixin" ||
      weixinLoginOwner.current?.scope !== imActionScope.current
    ) {
      weixinLoginOwner.current = null;
      return;
    }
    if (imBusy || !groupId) return;
    if (!weixinLoginStatus?.logged_in) {
      if (!shouldPollWeixinLogin(weixinLoginStatus)) weixinLoginOwner.current = null;
      return;
    }
    // Saving QR login configuration leaves the bridge disabled. This explicit
    // login intent, not that saved enabled flag, authorizes one subsequent start.
    if (!imStatus?.configured || imStatus.platform !== "weixin" || imStatus.running) {
      weixinLoginOwner.current = null;
      return;
    }
    const owner = weixinLoginOwner.current;
    const { isCurrent, canReload } = currentIMAction();
    void (async () => {
      setImBusy(true);
      setImConfigError(null);
      try {
        await api.runIMManagement(groupId, false, async () => {
          if (!isCurrent() || weixinLoginOwner.current !== owner) return;
          weixinLoginOwner.current = null;
          const resp = await api.startIMBridge(groupId);
          if (!isCurrent()) return;
          if (resp.ok) await loadIMStatus({ isCurrent, canReloadConfig: canReload });
          else
            setImConfigError({
              groupId,
              message: resp.error?.message || t("imBridge.weixinBridgeStartFailed"),
            });
        });
      } catch {
        if (isCurrent())
          setImConfigError({ groupId, message: t("imBridge.weixinBridgeStartFailed") });
      } finally {
        if (weixinLoginOwner.current === owner) weixinLoginOwner.current = null;
        if (isCurrent()) setImBusy(false);
      }
    })();
  }, [
    isOpen,
    groupId,
    imPlatform,
    imBusy,
    imStatus,
    loadIMStatus,
    weixinLoginStatus,
    currentIMAction,
    t,
  ]);

  const getCurrentIMConfigDraft = (): IMConfigDraft => ({
    botTokenEnv: imBotTokenEnv,
    appTokenEnv: imAppTokenEnv,
    mattermostUrl: imMattermostUrl,
    feishuDomain: imFeishuDomain,
    feishuAppId: imFeishuAppId,
    feishuAppSecret: imFeishuAppSecret,
    dingtalkAppKey: imDingtalkAppKey,
    dingtalkAppSecret: imDingtalkAppSecret,
    dingtalkRobotCode: imDingtalkRobotCode,
    wecomBotId: imWecomBotId,
    wecomSecret: imWecomSecret,
    weixinAccountId: imWeixinAccountId,
  });

  const applyIMConfigDraft = (draft: IMConfigDraft) => {
    setImBotTokenEnvState(draft.botTokenEnv);
    setImAppTokenEnv(draft.appTokenEnv);
    setImMattermostUrlState(draft.mattermostUrl);
    setImFeishuDomain(draft.feishuDomain);
    setImFeishuAppId(draft.feishuAppId);
    setImFeishuAppSecret(draft.feishuAppSecret);
    setImDingtalkAppKey(draft.dingtalkAppKey);
    setImDingtalkAppSecret(draft.dingtalkAppSecret);
    setImDingtalkRobotCode(draft.dingtalkRobotCode);
    setImWecomBotId(draft.wecomBotId);
    setImWecomSecret(draft.wecomSecret);
    setImWeixinAccountId(draft.weixinAccountId);
  };

  const getCurrentIMSaveRequest = () => ({
    groupId: String(groupId || ""),
    platform: imPlatform,
    ...getCurrentIMConfigDraft(),
  });

  const handlePlatformChange = (newPlatform: IMPlatform) => {
    if (newPlatform === imPlatform) return;
    weixinLoginOwner.current = null;
    if (imPlatform === "weixin" || newPlatform === "weixin") setImBusy(false);
    // User selection invalidates stale Mattermost reads; programmatic hydration is not a user edit.
    imPlatformSelectionSeq.current += 1;
    if (imPlatform === "mattermost" || newPlatform === "mattermost") imLoadSeq.current += 1;
    setImConfigError(null);

    // 1. Save current platform config to drafts
    setImConfigDrafts((prev) => ({ ...prev, [imPlatform]: getCurrentIMConfigDraft() }));

    // 2. Load new platform's cached draft (if exists)
    const cachedDraft = imConfigDrafts[newPlatform];
    if (cachedDraft) {
      applyIMConfigDraft(cachedDraft);
    } else {
      clearIMCredentialsDraft();
    }

    // 3. Set new platform
    setImPlatform(newPlatform);
  };

  const handleSaveIMConfig = async () => {
    if (!groupId) return;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    setImConfigError(null);
    try {
      await api.runIMManagement(groupId, imPlatform === "mattermost", async () => {
        if (!isCurrent()) return;
        const resp = await saveIMConfigDraft(getCurrentIMSaveRequest());
        if (!isCurrent()) return;
        if (resp.ok) {
          await loadIMStatus({ isCurrent, canReloadConfig: canReload });
        } else if (imPlatform === "mattermost") {
          setImConfigError({
            groupId,
            message: resp.error?.message || t("imBridge.mattermostConfigFailed"),
          });
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      if (imPlatform === "mattermost") {
        setImConfigError({ groupId, message: t("imBridge.mattermostConfigFailed") });
      }
      console.error("Failed to save IM config:", e);
    } finally {
      if (isCurrent()) {
        imMattermostBusy.current = false;
        setImBusy(false);
      }
    }
  };

  const handleRemoveIMConfig = async () => {
    if (!groupId) return;
    weixinLoginOwner.current = null;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    setImConfigError(null);
    try {
      await api.runIMManagement(groupId, imPlatform === "mattermost", async () => {
        if (!isCurrent()) return;
        const resp = await api.unsetIMConfig(groupId);
        if (!isCurrent()) return;
        if (!resp.ok && imPlatform === "mattermost") {
          setImConfigError({
            groupId,
            message: resp.error?.message || t("imBridge.mattermostConfigFailed"),
          });
        }
        if (resp.ok) {
          if (canReload()) {
            clearIMCredentialsDraft();
          }
          await loadIMStatus({ isCurrent, canReloadConfig: canReload });
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      if (imPlatform === "mattermost") {
        setImConfigError({ groupId, message: t("imBridge.mattermostConfigFailed") });
      }
      console.error("Failed to remove IM config:", e);
    } finally {
      if (isCurrent()) {
        imMattermostBusy.current = false;
        setImBusy(false);
      }
    }
  };

  const handleStartBridge = async () => {
    if (!groupId) return;
    weixinLoginOwner.current = null;
    if (!canStartIMBridge(imPlatform, !!weixinLoginStatus?.logged_in)) return;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    setImConfigError(null);
    try {
      await api.runIMManagement(groupId, imPlatform === "mattermost", async () => {
        if (!isCurrent()) return;
        // Preserve the draft after a failed Mattermost save; reloading would replace it with the old platform/config.
        if (imPlatform === "mattermost") {
          const saved = await saveIMConfigDraft(getCurrentIMSaveRequest());
          if (!isCurrent()) return;
          if (!saved.ok) {
            setImConfigError({
              groupId,
              message: saved.error?.message || t("imBridge.mattermostConfigFailed"),
            });
            return;
          }
        }
        const resp =
          imPlatform === "mattermost"
            ? await api.startIMBridge(groupId)
            : await saveAndStartIMBridge(getCurrentIMSaveRequest());
        if (!isCurrent()) return;
        await loadIMStatus({ isCurrent, canReloadConfig: canReload });
        if (!isCurrent()) return;
        if (!resp.ok && imPlatform === "mattermost") {
          setImConfigError({
            groupId,
            message: resp.error?.message || t("imBridge.mattermostConfigFailed"),
          });
        }
        if (!resp.ok && imPlatform === "weixin") {
          setImConfigError({
            groupId,
            message: resp.error?.message || t("imBridge.weixinBridgeStartFailed"),
          });
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      if (imPlatform === "mattermost") {
        setImConfigError({ groupId, message: t("imBridge.mattermostConfigFailed") });
      }
      if (imPlatform === "weixin") {
        setImConfigError({ groupId, message: t("imBridge.weixinBridgeStartFailed") });
      } else {
        console.error("Failed to start bridge:", e);
      }
    } finally {
      if (isCurrent()) {
        imMattermostBusy.current = false;
        setImBusy(false);
      }
    }
  };

  const handleStopBridge = async () => {
    if (!groupId) return;
    weixinLoginOwner.current = null;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    if (imPlatform === "mattermost") setImConfigError(null);
    try {
      await api.runIMManagement(groupId, imPlatform === "mattermost", async () => {
        if (!isCurrent()) return;
        const resp = await api.stopIMBridge(groupId);
        if (!isCurrent()) return;
        if (!resp.ok && imPlatform === "mattermost") {
          setImConfigError({
            groupId,
            message: resp.error?.message || t("imBridge.mattermostConfigFailed"),
          });
          return;
        }
        await loadIMStatus({ isCurrent, canReloadConfig: canReload });
      });
    } catch (e) {
      if (!isCurrent()) return;
      if (imPlatform === "mattermost") {
        setImConfigError({ groupId, message: t("imBridge.mattermostConfigFailed") });
      }
      console.error("Failed to stop bridge:", e);
    } finally {
      if (isCurrent()) {
        imMattermostBusy.current = false;
        setImBusy(false);
      }
    }
  };

  const handleStartWeixinLogin = async () => {
    if (!groupId) return;
    weixinLoginOwner.current = null;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    try {
      await api.runIMManagement(groupId, false, async () => {
        if (!isCurrent()) return;
        const saveResp = await saveIMConfigDraft(getCurrentIMSaveRequest());
        if (!isCurrent()) return;
        if (!saveResp.ok) {
          setWeixinLoginStatus(
            toWeixinErrorStatus(saveResp.error?.message || t("imBridge.weixinStartFailed")),
          );
          return;
        }
        await loadIMStatus({ isCurrent, canReloadConfig: canReload });
        if (!isCurrent()) return;
        const resp = await api.startWeixinLogin(groupId);
        if (!isCurrent()) return;
        if (resp.ok) {
          weixinLoginOwner.current = { scope: imActionScope.current };
          setWeixinLoginStatus(resp.result ?? null);
        } else {
          setWeixinLoginStatus(
            toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinStartFailed")),
          );
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinStartFailed")));
      console.error("Failed to start weixin login:", e);
    } finally {
      if (isCurrent()) setImBusy(false);
    }
  };

  const handleLogoutWeixin = async () => {
    if (!groupId) return;
    weixinLoginOwner.current = null;
    const { isCurrent, canReload } = currentIMAction();
    setImBusy(true);
    try {
      await api.runIMManagement(groupId, false, async () => {
        if (!isCurrent()) return;
        const resp = await api.logoutWeixin(groupId);
        if (!isCurrent()) return;
        if (resp.ok) {
          setWeixinLoginStatus(resp.result ?? null);
          await loadIMStatus({ isCurrent, canReloadConfig: canReload });
        } else {
          setWeixinLoginStatus(
            toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinLogoutFailed")),
          );
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinLogoutFailed")));
      console.error("Failed to logout weixin:", e);
    } finally {
      if (isCurrent()) setImBusy(false);
    }
  };

  const handleVerifyWeixin = async (verifyCode: string) => {
    if (!groupId) return;
    const { isCurrent } = currentIMAction();
    setImBusy(true);
    try {
      await api.runIMManagement(groupId, false, async () => {
        if (!isCurrent()) return;
        const resp = await api.verifyWeixinLogin(groupId, verifyCode);
        if (!isCurrent()) return;
        if (resp.ok) {
          setWeixinLoginStatus(resp.result ?? null);
        } else {
          setWeixinLoginStatus(
            toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinVerifyFailed")),
          );
        }
      });
    } catch (e) {
      if (!isCurrent()) return;
      setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinVerifyFailed")));
      console.error("Failed to verify weixin login:", e);
    } finally {
      if (isCurrent()) setImBusy(false);
    }
  };

  const setImBotTokenEnv = (value: string) => {
    if (imPlatform === "mattermost") {
      imMattermostEditSeq.current += 1;
    }
    setImBotTokenEnvState(value);
  };

  const setImMattermostUrl = (value: string) => {
    imMattermostEditSeq.current += 1;
    setImMattermostUrlState(value);
  };

  return {
    imStatus,
    imConfigError:
      imConfigError && imConfigError.groupId === groupId ? imConfigError.message : undefined,
    imPlatform,
    handlePlatformChange,
    imBotTokenEnv,
    setImBotTokenEnv,
    imAppTokenEnv,
    setImAppTokenEnv,
    imMattermostUrl,
    setImMattermostUrl,
    imFeishuAppId,
    setImFeishuAppId,
    imFeishuAppSecret,
    setImFeishuAppSecret,
    imFeishuDomain,
    setImFeishuDomain,
    imDingtalkAppKey,
    setImDingtalkAppKey,
    imDingtalkAppSecret,
    setImDingtalkAppSecret,
    imDingtalkRobotCode,
    setImDingtalkRobotCode,
    imWecomBotId,
    setImWecomBotId,
    imWecomSecret,
    setImWecomSecret,
    imWeixinAccountId,
    setImWeixinAccountId,
    weixinLoginStatus,
    handleStartWeixinLogin,
    handleVerifyWeixin,
    handleLogoutWeixin,
    imBusy,
    handleSaveIMConfig,
    handleRemoveIMConfig,
    handleStartBridge,
    handleStopBridge,
  };
}
