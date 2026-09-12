use super::{AsrError, Provider};
use cccc_core::HomeLayout;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;

// Deliberately no Debug: credentials must never enter diagnostics or API payloads.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Config {
    pub region: String,
    pub workspace_id: String,
    pub model: String,
    pub resource_id: String,
    pub auth_mode: String,
    pub api_key: String,
    pub app_id: String,
    pub access_token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            region: "beijing".into(),
            workspace_id: String::new(),
            model: "fun-asr-realtime".into(),
            resource_id: "volc.seedasr.sauc.duration".into(),
            auth_mode: "api_key".into(),
            api_key: String::new(),
            app_id: String::new(),
            access_token: String::new(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Update {
    pub region: Option<String>,
    pub workspace_id: Option<String>,
    pub model: Option<String>,
    pub resource_id: Option<String>,
    pub auth_mode: Option<String>,
    pub api_key: Option<String>,
    pub app_id: Option<String>,
    pub access_token: Option<String>,
    pub clear_credentials: bool,
}

impl Config {
    pub fn validate(&self, provider: Provider) -> Result<(), AsrError> {
        if !["beijing", "singapore"].contains(&self.region.as_str())
            || !["fun-asr-realtime", "paraformer-realtime-v2"].contains(&self.model.as_str())
            || !["api_key", "app_token"].contains(&self.auth_mode.as_str())
            || (provider == Provider::Bailian && self.auth_mode != "api_key")
            || ![
                "volc.seedasr.sauc.duration",
                "volc.seedasr.sauc.concurrent",
                "volc.bigasr.sauc.duration",
                "volc.bigasr.sauc.concurrent",
            ]
            .contains(&self.resource_id.as_str())
            || self.workspace_id.len() > 64
            || !self
                .workspace_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(AsrError::new(
                "external_asr_invalid_config",
                "Invalid external ASR model, region or authentication configuration",
            ));
        }
        if [&self.api_key, &self.app_id, &self.access_token]
            .iter()
            .any(|s| s.len() > 4096 || s.bytes().any(|b| !b.is_ascii_graphic()))
        {
            return Err(AsrError::new(
                "external_asr_invalid_config",
                "ASR credentials must contain printable ASCII without whitespace",
            ));
        }
        Ok(())
    }

    pub fn configured(&self, provider: Provider) -> bool {
        self.validate(provider).is_ok()
            && if provider == Provider::Volcengine && self.auth_mode == "app_token" {
                !self.app_id.is_empty() && !self.access_token.is_empty()
            } else {
                !self.api_key.is_empty()
            }
    }

    pub fn model_id(&self, provider: Provider) -> &str {
        if provider == Provider::Bailian {
            &self.model
        } else {
            &self.resource_id
        }
    }

    pub fn endpoint(&self, provider: Provider) -> String {
        if provider == Provider::Volcengine {
            return "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async".into();
        }
        let host = match (self.region.as_str(), self.workspace_id.is_empty()) {
            ("singapore", true) => "dashscope-intl.aliyuncs.com".into(),
            ("singapore", false) => {
                format!("{}.ap-southeast-1.maas.aliyuncs.com", self.workspace_id)
            }
            (_, true) => "dashscope.aliyuncs.com".into(),
            (_, false) => format!("{}.cn-beijing.maas.aliyuncs.com", self.workspace_id),
        };
        format!("wss://{host}/api-ws/v1/inference")
    }

    pub fn public(&self, provider: Provider) -> Value {
        json!({"provider":provider.id(),"region":self.region,"workspace_id":self.workspace_id,
            "model":self.model,"resource_id":self.resource_id,"auth_mode":self.auth_mode,
            "has_api_key":!self.api_key.is_empty(),"has_app_id":!self.app_id.is_empty(),
            "has_access_token":!self.access_token.is_empty(),"configured":self.configured(provider)})
    }
}

fn path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join("config/voice-asr-providers.json")
}

fn read(home: &HomeLayout) -> Result<BTreeMap<String, Config>, AsrError> {
    let file = match std::fs::File::open(path(home)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(_) => return Err(config_error()),
    };
    let mut bytes = Vec::new();
    file.take(32_769)
        .read_to_end(&mut bytes)
        .map_err(|_| config_error())?;
    if bytes.len() > 32_768 {
        return Err(config_error());
    }
    serde_json::from_slice(&bytes).map_err(|_| config_error())
}

pub(super) fn load(home: &HomeLayout, provider: Provider) -> Result<Config, AsrError> {
    Ok(read(home)?.remove(provider.id()).unwrap_or_default())
}

pub(super) fn save(
    home: &HomeLayout,
    provider: Provider,
    patch: Update,
) -> Result<Value, AsrError> {
    let mut validation_error = None;
    let result = cccc_core::fs::with_exclusive_lock(&path(home).with_extension("lock"), || {
        let mut all = read(home).map_err(|_| std::io::Error::other("invalid ASR configuration"))?;
        let value = all.entry(provider.id().into()).or_default();
        if patch.clear_credentials {
            value.api_key.clear();
            value.app_id.clear();
            value.access_token.clear();
        }
        for (target, incoming) in [
            (&mut value.region, patch.region),
            (&mut value.workspace_id, patch.workspace_id),
            (&mut value.model, patch.model),
            (&mut value.resource_id, patch.resource_id),
            (&mut value.auth_mode, patch.auth_mode),
        ] {
            if let Some(text) = incoming {
                *target = text.trim().to_owned();
            }
        }
        for (target, incoming) in [
            (&mut value.api_key, patch.api_key),
            (&mut value.app_id, patch.app_id),
            (&mut value.access_token, patch.access_token),
        ] {
            if let Some(text) = incoming.filter(|s| !s.trim().is_empty()) {
                *target = text.trim().to_owned();
            }
        }
        value.validate(provider).map_err(|error| {
            validation_error = Some(error);
            std::io::Error::other("invalid ASR configuration")
        })?;
        let public = value.public(provider);
        // atomic_write uses an owner-only temporary file and an atomic rename.
        cccc_core::fs::write_secret_json(&path(home), &all)?;
        Ok(public)
    });
    result.map_err(|_| validation_error.unwrap_or_else(config_error))
}

fn config_error() -> AsrError {
    AsrError::new(
        "external_asr_config_error",
        "External ASR configuration could not be read or saved",
    )
}
