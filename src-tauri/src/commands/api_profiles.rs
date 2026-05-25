use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

use crate::core::{log_sanitize, skill_store::SkillStore};

const API_PROFILES_SETTING: &str = "api_profiles";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub model_id: Option<String>,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveApiProfileRequest {
    pub id: Option<String>,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub model_id: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfileIdRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetApiProfileEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListApiModelsForConfigRequest {
    pub provider: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestApiProfileResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveApiConnectionStatus {
    pub status: String,
    pub message: String,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListApiProfileModelsResult {
    pub supported: bool,
    pub models: Vec<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
struct TestOpenAiRequest {
    model: String,
    messages: Vec<TestChatMessage>,
    temperature: f32,
    stream: bool,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct TestOpenAiResponse {
    choices: Vec<TestOpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct TestOpenAiChoice {
    message: TestChatMessage,
}

#[derive(Debug, Serialize)]
struct TestOllamaRequest {
    model: String,
    messages: Vec<TestChatMessage>,
    stream: bool,
    options: TestOllamaOptions,
}

#[derive(Debug, Serialize)]
struct TestOllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Deserialize)]
struct TestOllamaResponse {
    message: Option<TestChatMessage>,
    response: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Option<Vec<OpenAiModelItem>>,
    models: Option<Vec<LlamaCppModelItem>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelItem {
    id: String,
}

#[derive(Debug, Deserialize)]
struct LlamaCppModelItem {
    name: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelItem>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelItem {
    name: String,
}

fn read_profiles(store: &SkillStore) -> Result<Vec<ApiProfile>, String> {
    let Some(text) = store
        .get_setting(API_PROFILES_SETTING)
        .map_err(|err| format!("读取 API 配置失败：{err}"))?
    else {
        return Ok(Vec::new());
    };

    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&text).map_err(|err| format!("解析 API 配置失败：{err}"))
}

fn write_profiles(store: &SkillStore, profiles: &[ApiProfile]) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(profiles).map_err(|err| format!("序列化 API 配置失败：{err}"))?;

    store
        .set_setting(API_PROFILES_SETTING, &text)
        .map_err(|err| format!("写入 API 配置失败：{err}"))
}

fn safe_id_part(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();

    cleaned
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn new_profile_id(name: &str) -> String {
    let safe_name = safe_id_part(name);
    let suffix = Utc::now().timestamp_millis();

    if safe_name.is_empty() {
        format!("api-{suffix}")
    } else {
        format!("{safe_name}-{suffix}")
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn actual_model(profile: &ApiProfile) -> String {
    profile
        .model_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&profile.model)
        .to_string()
}

fn build_openai_chat_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');

    if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn build_openai_models_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');

    if let Some(prefix) = base.strip_suffix("/chat/completions") {
        format!("{prefix}/models")
    } else if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    }
}

fn build_ollama_chat_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');

    if base.ends_with("/api/chat") {
        base.to_string()
    } else {
        format!("{base}/api/chat")
    }
}

fn build_ollama_tags_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');

    if let Some(prefix) = base.strip_suffix("/api/chat") {
        format!("{prefix}/api/tags")
    } else {
        format!("{base}/api/tags")
    }
}

fn sanitize_message(message: impl AsRef<str>) -> String {
    log_sanitize::sanitize(message.as_ref())
}

fn build_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|err| format!("创建 API 客户端失败：{err}"))
}

#[tauri::command]
pub fn list_api_profiles(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ApiProfile>, String> {
    read_profiles(store.inner())
}

#[tauri::command]
pub fn save_api_profile(
    request: SaveApiProfileRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ApiProfile, String> {
    let mut profiles = read_profiles(store.inner())?;
    let now = Utc::now().timestamp();

    let SaveApiProfileRequest {
        id,
        name,
        provider,
        base_url,
        model,
        model_id,
        api_key,
        enabled,
        note,
    } = request;

    let name = name.trim().to_string();
    let provider = provider.trim().to_string();
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let model_id = trim_optional(model_id);
    let model = {
        let trimmed = model.trim().to_string();
        if trimmed.is_empty() {
            model_id.clone().unwrap_or_default()
        } else {
            trimmed
        }
    };

    if name.is_empty() {
        return Err("API 名称不能为空".to_string());
    }
    if provider.is_empty() {
        return Err("供应商不能为空".to_string());
    }
    if base_url.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }

    let id = id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| new_profile_id(&name));
    let existing_index = profiles.iter().position(|profile| profile.id == id);
    let created_at = existing_index
        .and_then(|index| profiles.get(index).map(|profile| profile.created_at))
        .unwrap_or(now);
    let enabled = enabled.unwrap_or_else(|| {
        existing_index
            .and_then(|index| profiles.get(index).map(|profile| profile.enabled))
            .unwrap_or_else(|| profiles.is_empty())
    });

    if enabled {
        for item in profiles.iter_mut() {
            item.enabled = false;
        }
    }

    let profile = ApiProfile {
        id: id.clone(),
        name,
        provider,
        base_url,
        model,
        model_id,
        api_key: trim_optional(api_key),
        enabled,
        note: trim_optional(note),
        created_at,
        updated_at: now,
    };

    match existing_index {
        Some(index) => profiles[index] = profile.clone(),
        None => profiles.push(profile.clone()),
    }

    write_profiles(store.inner(), &profiles)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_api_profile(
    request: ApiProfileIdRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let mut profiles = read_profiles(store.inner())?;
    profiles.retain(|profile| profile.id != request.id);
    write_profiles(store.inner(), &profiles)
}

#[tauri::command]
pub fn set_api_profile_enabled(
    request: SetApiProfileEnabledRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ApiProfile, String> {
    let mut profiles = read_profiles(store.inner())?;

    let Some(index) = profiles.iter().position(|profile| profile.id == request.id) else {
        return Err("API 配置不存在".to_string());
    };

    if request.enabled {
        for profile in profiles.iter_mut() {
            profile.enabled = false;
        }
    }

    profiles[index].enabled = request.enabled;
    profiles[index].updated_at = Utc::now().timestamp();

    let profile = profiles[index].clone();
    write_profiles(store.inner(), &profiles)?;

    Ok(profile)
}

#[tauri::command]
pub async fn test_api_profile(
    request: ApiProfileIdRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<TestApiProfileResult, String> {
    let profiles = read_profiles(store.inner())?;

    let Some(profile) = profiles.into_iter().find(|item| item.id == request.id) else {
        return Err("API 配置不存在".to_string());
    };

    let client = build_client(30)?;
    let mut model = actual_model(&profile);

    if model.trim().is_empty() {
        let result = list_models_by_config(
            &profile.provider,
            &profile.base_url,
            profile.api_key.as_deref(),
        )
        .await?;

        if let Some(first_model) = result
            .models
            .into_iter()
            .find(|item| !item.trim().is_empty())
        {
            model = first_model;
        } else {
            return Ok(TestApiProfileResult {
                ok: false,
                message: format!("未指定模型，且自动获取模型失败：{}", result.message),
            });
        }
    }

    let test_prompt = "Reply with only: OK";

    if profile.provider == "ollama" {
        return test_ollama_profile(&client, &profile, model, test_prompt).await;
    }

    test_openai_compatible_profile(&client, &profile, model, test_prompt).await
}

async fn test_ollama_profile(
    client: &reqwest::Client,
    profile: &ApiProfile,
    model: String,
    test_prompt: &str,
) -> Result<TestApiProfileResult, String> {
    let url = build_ollama_chat_url(&profile.base_url);
    let body = TestOllamaRequest {
        model,
        stream: false,
        options: TestOllamaOptions {
            temperature: 0.0,
            num_predict: 16,
        },
        messages: vec![TestChatMessage {
            role: "user".to_string(),
            content: test_prompt.to_string(),
        }],
    };

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("连接 Ollama API 失败：{err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = sanitize_message(response.text().await.unwrap_or_default());
        return Ok(TestApiProfileResult {
            ok: false,
            message: format!("Ollama 请求失败：{status} {body}"),
        });
    }

    let data: TestOllamaResponse = response
        .json()
        .await
        .map_err(|err| format!("解析 Ollama 响应失败：{err}"))?;
    let content = data
        .message
        .map(|message| message.content)
        .or(data.response)
        .unwrap_or_default();

    Ok(TestApiProfileResult {
        ok: true,
        message: if content.trim().is_empty() {
            "连接成功".to_string()
        } else {
            format!("连接成功：{}", content.trim())
        },
    })
}

async fn test_openai_compatible_profile(
    client: &reqwest::Client,
    profile: &ApiProfile,
    model: String,
    test_prompt: &str,
) -> Result<TestApiProfileResult, String> {
    let url = build_openai_chat_url(&profile.base_url);
    let body = TestOpenAiRequest {
        model,
        temperature: 0.0,
        stream: false,
        max_tokens: 16,
        messages: vec![TestChatMessage {
            role: "user".to_string(),
            content: test_prompt.to_string(),
        }],
    };

    let mut request = client.post(&url).json(&body);
    if let Some(api_key) = profile.api_key.as_ref().filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(api_key);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("连接 API 失败：{err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = sanitize_message(response.text().await.unwrap_or_default());
        return Ok(TestApiProfileResult {
            ok: false,
            message: format!("OpenAI 兼容接口请求失败：{status} {body}"),
        });
    }

    let data: TestOpenAiResponse = response
        .json()
        .await
        .map_err(|err| format!("解析 API 响应失败：{err}"))?;
    let content = data
        .choices
        .first()
        .map(|choice| choice.message.content.trim().to_string())
        .unwrap_or_default();

    Ok(TestApiProfileResult {
        ok: true,
        message: if content.is_empty() {
            "连接成功".to_string()
        } else {
            format!("连接成功：{content}")
        },
    })
}

#[tauri::command]
pub async fn check_active_api_connection(
    store: State<'_, Arc<SkillStore>>,
) -> Result<ActiveApiConnectionStatus, String> {
    let profiles = read_profiles(store.inner())?;

    let Some(profile) = profiles.into_iter().find(|item| item.enabled) else {
        return Ok(ActiveApiConnectionStatus {
            status: "unknown".to_string(),
            message: "没有启用的 API".to_string(),
            profile_id: None,
            profile_name: None,
        });
    };

    let client = build_client(5)?;
    let url = if profile.provider == "ollama" {
        build_ollama_tags_url(&profile.base_url)
    } else if profile.provider == "lm-studio-api" || profile.provider == "lm-studio" {
        let base = profile.base_url.trim_end_matches('/');
        format!("{base}/api/v1/models")
    } else {
        build_openai_models_url(&profile.base_url)
    };

    let mut request = client.get(&url);
    if profile.provider != "ollama" {
        if let Some(api_key) = profile.api_key.as_ref().filter(|value| !value.trim().is_empty()) {
            request = request.bearer_auth(api_key);
        }
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => Ok(ActiveApiConnectionStatus {
            status: "ok".to_string(),
            message: format!("{} 连接正常", profile.name),
            profile_id: Some(profile.id),
            profile_name: Some(profile.name),
        }),
        Ok(response) => Ok(ActiveApiConnectionStatus {
            status: "failed".to_string(),
            message: format!("{} 连接失败：{}", profile.name, response.status()),
            profile_id: Some(profile.id),
            profile_name: Some(profile.name),
        }),
        Err(err) => Ok(ActiveApiConnectionStatus {
            status: "failed".to_string(),
            message: format!("{} 连接失败：{}", profile.name, sanitize_message(err.to_string())),
            profile_id: Some(profile.id),
            profile_name: Some(profile.name),
        }),
    }
}

async fn list_models_by_config(
    provider: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<ListApiProfileModelsResult, String> {
    if base_url.trim().is_empty() {
        return Ok(ListApiProfileModelsResult {
            supported: false,
            models: Vec::new(),
            message: "请先填写地址".to_string(),
        });
    }

    let client = build_client(10)?;

    if provider == "ollama" {
        let url = build_ollama_tags_url(base_url);
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|err| format!("无法获取 Ollama 模型列表：{err}"))?;
        let status = response.status();

        if !status.is_success() {
            let body = sanitize_message(response.text().await.unwrap_or_default());
            return Ok(ListApiProfileModelsResult {
                supported: true,
                models: Vec::new(),
                message: format!("获取失败：{status} {body}"),
            });
        }

        let data: OllamaTagsResponse = response
            .json()
            .await
            .map_err(|err| format!("解析 Ollama 模型列表失败：{err}"))?;
        let mut models = data.models.into_iter().map(|item| item.name).collect::<Vec<_>>();
        models.sort();
        models.dedup();

        return Ok(ListApiProfileModelsResult {
            supported: true,
            message: if models.is_empty() {
                "没有发现模型".to_string()
            } else {
                format!("发现 {} 个模型", models.len())
            },
            models,
        });
    }

    if provider == "anthropic-compatible" {
        return Ok(ListApiProfileModelsResult {
            supported: false,
            models: Vec::new(),
            message: "Anthropic 兼容接口通常不提供模型列表，请手动填写指定模型".to_string(),
        });
    }

    let url = if provider == "lm-studio-api" || provider == "lm-studio" {
        let base = base_url.trim_end_matches('/');
        format!("{base}/api/v1/models")
    } else {
        build_openai_models_url(base_url)
    };

    let mut request = client.get(&url);
    if let Some(key) = api_key.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(key);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("无法获取模型列表：{err}"))?;
    let status = response.status();

    if !status.is_success() {
        let body = sanitize_message(response.text().await.unwrap_or_default());
        return Ok(ListApiProfileModelsResult {
            supported: false,
            models: Vec::new(),
            message: format!("该供应商可能不支持模型列表接口：{status} {body}"),
        });
    }

    let data: OpenAiModelsResponse = response
        .json()
        .await
        .map_err(|err| format!("解析模型列表失败：{err}"))?;
    let mut models = Vec::new();

    if let Some(items) = data.data {
        models.extend(items.into_iter().map(|item| item.id));
    }
    if let Some(items) = data.models {
        models.extend(items.into_iter().filter_map(|item| item.name.or(item.model)));
    }

    models.sort();
    models.dedup();

    Ok(ListApiProfileModelsResult {
        supported: true,
        message: if models.is_empty() {
            "没有发现模型".to_string()
        } else {
            format!("发现 {} 个模型", models.len())
        },
        models,
    })
}

#[tauri::command]
pub async fn list_api_profile_models(
    request: ApiProfileIdRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ListApiProfileModelsResult, String> {
    let profiles = read_profiles(store.inner())?;

    let Some(profile) = profiles.into_iter().find(|item| item.id == request.id) else {
        return Err("API 配置不存在".to_string());
    };

    list_models_by_config(
        &profile.provider,
        &profile.base_url,
        profile.api_key.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn list_api_models_for_config(
    request: ListApiModelsForConfigRequest,
) -> Result<ListApiProfileModelsResult, String> {
    list_models_by_config(
        &request.provider,
        &request.base_url,
        request.api_key.as_deref(),
    )
    .await
}
