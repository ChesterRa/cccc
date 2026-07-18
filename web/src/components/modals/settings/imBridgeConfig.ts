import type { IMPlatform } from "../../../types";
import type { ApiResponse } from "../../../services/api";
import * as api from "../../../services/api";

export type IMConfigDraft = {
  botTokenEnv: string;
  appTokenEnv: string;
  feishuDomain: string;
  feishuAppId: string;
  feishuAppSecret: string;
  dingtalkAppKey: string;
  dingtalkAppSecret: string;
  dingtalkRobotCode: string;
  wecomBotId: string;
  wecomSecret: string;
  weixinAccountId: string;
};

export type IMConfigSaveRequest = IMConfigDraft & { groupId: string; platform: IMPlatform };

function toIMConfigExtra(config: IMConfigDraft) {
  return {
    feishu_domain: config.feishuDomain,
    feishu_app_id: config.feishuAppId,
    feishu_app_secret: config.feishuAppSecret,
    dingtalk_app_key: config.dingtalkAppKey,
    dingtalk_app_secret: config.dingtalkAppSecret,
    dingtalk_robot_code: config.dingtalkRobotCode,
    wecom_bot_id: config.wecomBotId,
    wecom_secret: config.wecomSecret,
    weixin_account_id: config.weixinAccountId,
  };
}

export function saveIMConfigDraft(config: IMConfigSaveRequest) {
  return api.setIMConfig(
    config.groupId,
    config.platform,
    config.botTokenEnv,
    config.appTokenEnv,
    toIMConfigExtra(config),
  );
}

export async function saveAndStartIMBridge(
  config: IMConfigSaveRequest,
): Promise<ApiResponse<unknown>> {
  const saveResp = await saveIMConfigDraft(config);
  if (!saveResp.ok) return saveResp;
  return api.startIMBridge(config.groupId);
}
