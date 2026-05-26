use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

use crate::commands::api_profiles::{load_api_profiles, ApiProfile};
use crate::core::{log_sanitize, skill_store::SkillStore};

const TRANSLATION_API_PROFILE_SETTING: &str = "translation_api_profile_id";
const TRANSLATION_API_DISABLED: &str = "__disabled__";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTextRequest {
    pub text: String,
    pub target_lang: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationApiConnectionStatus {
    pub status: String,
    pub message: String,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    stream: bool,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionResponse {
    choices: Vec<OpenAiChatChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<ChatMessage>,
    response: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AutoOpenAiModelsResponse {
    data: Option<Vec<AutoOpenAiModelItem>>,
    models: Option<Vec<AutoLlamaCppModelItem>>,
}

#[derive(Debug, Deserialize)]
struct AutoOpenAiModelItem {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AutoLlamaCppModelItem {
    name: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AutoOllamaTagsResponse {
    models: Vec<AutoOllamaModelItem>,
}

#[derive(Debug, Deserialize)]
struct AutoOllamaModelItem {
    name: String,
}

fn actual_model(profile: &ApiProfile) -> String {
    profile
        .model_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&profile.model)
        .to_string()
}

fn build_openai_url(base_url: &str) -> String {
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

fn build_ollama_url(base_url: &str) -> String {
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

fn select_translation_profile(
    profiles: &[ApiProfile],
    preferred_id: Option<&str>,
) -> Option<ApiProfile> {
    if let Some(preferred_id) = preferred_id
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != TRANSLATION_API_DISABLED)
    {
        if let Some(profile) = profiles.iter().find(|profile| profile.id == preferred_id) {
            return Some(profile.clone());
        }
    }

    profiles.iter().find(|profile| profile.enabled).cloned()
}

fn get_translation_api_profile(store: &SkillStore) -> Result<ApiProfile, String> {
    let profiles = load_api_profiles(store)?;
    let preferred_id = store
        .get_setting(TRANSLATION_API_PROFILE_SETTING)
        .map_err(|err| format!("读取翻译默认 API 设置失败：{err}"))?;

    if preferred_id.as_deref() == Some(TRANSLATION_API_DISABLED) {
        return Err("翻译 API 已关闭。请先在 API 管理中选择一个翻译 API。".to_string());
    }

    if let Some(profile) = select_translation_profile(&profiles, preferred_id.as_deref()) {
        return Ok(profile);
    }

    if profiles.is_empty() {
        return Err("没有可用的 API。请先在 API 管理中添加并启用一个模型 API。".to_string());
    }

    Err("没有可用于翻译的 API。请先在 API 管理中启用默认 API，或指定翻译 API。".to_string())
}

fn failed_translation_api_status(message: String) -> TranslationApiConnectionStatus {
    TranslationApiConnectionStatus {
        status: "failed".to_string(),
        message,
        profile_id: None,
        profile_name: None,
    }
}

async fn resolve_model(profile: &ApiProfile) -> Result<String, String> {
    let configured = actual_model(profile);
    if !configured.trim().is_empty() {
        return Ok(configured);
    }

    let client = build_client(10)?;

    if profile.provider == "ollama" {
        let url = build_ollama_tags_url(&profile.base_url);
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|err| format!("未指定模型，且无法获取 Ollama 模型列表：{err}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = sanitize_message(response.text().await.unwrap_or_default());
            return Err(format!(
                "未指定模型，且获取 Ollama 模型列表失败：{status} {body}"
            ));
        }

        let data: AutoOllamaTagsResponse = response
            .json()
            .await
            .map_err(|err| format!("解析 Ollama 模型列表失败：{err}"))?;

        return data
            .models
            .into_iter()
            .map(|item| item.name)
            .find(|name| !name.trim().is_empty())
            .ok_or_else(|| "未指定模型，且 Ollama 没有返回可用模型".to_string());
    }

    let url = if profile.provider == "lm-studio-api" || profile.provider == "lm-studio" {
        let base = profile.base_url.trim_end_matches('/');
        format!("{base}/api/v1/models")
    } else {
        build_openai_models_url(&profile.base_url)
    };

    let mut request = client.get(&url);
    if let Some(api_key) = profile
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        request = request.bearer_auth(api_key);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("未指定模型，且无法获取模型列表：{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = sanitize_message(response.text().await.unwrap_or_default());
        return Err(format!("未指定模型，且获取模型列表失败：{status} {body}"));
    }

    let data: AutoOpenAiModelsResponse = response
        .json()
        .await
        .map_err(|err| format!("解析模型列表失败：{err}"))?;

    if let Some(items) = data.data {
        if let Some(model) = items
            .into_iter()
            .map(|item| item.id)
            .find(|id| !id.trim().is_empty())
        {
            return Ok(model);
        }
    }

    if let Some(items) = data.models {
        if let Some(model) = items
            .into_iter()
            .filter_map(|item| item.name.or(item.model))
            .find(|name| !name.trim().is_empty())
        {
            return Ok(model);
        }
    }

    Err("未指定模型，且没有发现可用模型".to_string())
}

fn workspace_term_for(target_lang: &str) -> &'static str {
    let lower = target_lang.to_ascii_lowercase();
    if target_lang.contains('繁') || lower.contains("traditional") || lower.contains("zh-tw") {
        "工作區"
    } else if lower.contains("english") || lower == "en" || lower.starts_with("en-") {
        "workspace"
    } else {
        "工作区"
    }
}

fn build_prompt(text: &str, target_lang: &str) -> String {
    let workspace_term = workspace_term_for(target_lang);
    format!(
        r#"Translate the following AI agent skill content into {target_lang}.

Rules:
- Preserve Markdown structure.
- Preserve code blocks exactly.
- Do not translate commands, file paths, variable names, JSON/YAML keys, or identifiers.
- Do not translate or alter placeholders matching [[SMT0]], [[SMT1]], or similar numbered SMT placeholders.
- Translate natural-language explanations, titles, descriptions, and instructions.
- Product glossary: keep "Skill", "Agent", "Preset", and "API" as product terms; translate "workspace" consistently as "{workspace_term}".
- Return only the translated content.

Content:
{text}"#
    )
}

async fn send_openai_compatible_request(
    profile: &ApiProfile,
    prompt: String,
    max_tokens: u32,
) -> Result<String, String> {
    let url = build_openai_url(&profile.base_url);
    let body = OpenAiChatCompletionRequest {
        model: resolve_model(profile).await?,
        temperature: 0.2,
        stream: false,
        max_tokens,
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a careful technical translator for AI agent skill documentation."
                    .to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
    };
    let client = build_client(60)?;
    let mut last_error: Option<String> = None;

    for attempt in 1..=3 {
        let mut request = client.post(&url).json(&body);
        if let Some(api_key) = profile
            .api_key
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.bearer_auth(api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|err| format!("连接 API「{}」失败：{err}", profile.name))?;
        let status = response.status();

        if status.is_success() {
            let data: OpenAiChatCompletionResponse = response
                .json()
                .await
                .map_err(|err| format!("解析 OpenAI 兼容接口响应失败：{err}"))?;
            return Ok(data
                .choices
                .first()
                .map(|choice| choice.message.content.trim().to_string())
                .unwrap_or_default());
        }

        let body = sanitize_message(response.text().await.unwrap_or_default());
        last_error = Some(format!(
            "API「{}」请求失败：{status} {body}；请求地址：{url}；尝试次数：{attempt}/3",
            profile.name
        ));
        if status.as_u16() == 502 || status.as_u16() == 503 || status.as_u16() == 504 {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            continue;
        }
        break;
    }

    Err(last_error.unwrap_or_else(|| "API 请求失败，且没有返回具体错误".to_string()))
}

async fn send_ollama_request(
    profile: &ApiProfile,
    prompt: String,
    max_tokens: u32,
) -> Result<String, String> {
    let url = build_ollama_url(&profile.base_url);
    let body = OllamaChatRequest {
        model: resolve_model(profile).await?,
        stream: false,
        options: OllamaOptions {
            temperature: 0.2,
            num_predict: max_tokens,
        },
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a careful technical translator for AI agent skill documentation."
                    .to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
    };
    let client = build_client(60)?;
    let mut last_error: Option<String> = None;

    for attempt in 1..=3 {
        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| format!("连接 Ollama API「{}」失败：{err}", profile.name))?;
        let status = response.status();

        if status.is_success() {
            let data: OllamaChatResponse = response
                .json()
                .await
                .map_err(|err| format!("解析 Ollama 响应失败：{err}"))?;
            return Ok(data
                .message
                .map(|message| message.content.trim().to_string())
                .or_else(|| data.response.map(|value| value.trim().to_string()))
                .unwrap_or_default());
        }

        let body = sanitize_message(response.text().await.unwrap_or_default());
        last_error = Some(format!(
            "Ollama API「{}」请求失败：{status} {body}；请求地址：{url}；尝试次数：{attempt}/3",
            profile.name
        ));
        if status.as_u16() == 502 || status.as_u16() == 503 || status.as_u16() == 504 {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            continue;
        }
        break;
    }

    Err(last_error.unwrap_or_else(|| "Ollama 请求失败，且没有返回具体错误".to_string()))
}

#[tauri::command]
pub async fn check_translation_api_connection(
    store: State<'_, Arc<SkillStore>>,
) -> Result<TranslationApiConnectionStatus, String> {
    let profile = match get_translation_api_profile(store.inner().as_ref()) {
        Ok(profile) => profile,
        Err(message) => return Ok(failed_translation_api_status(message)),
    };

    if profile.provider == "anthropic-compatible" {
        return Ok(TranslationApiConnectionStatus {
            status: "failed".to_string(),
            message:
                "Anthropic 兼容翻译请求还没接入，请先使用 OpenAI 兼容 / LM Studio API / llama.cpp / Ollama"
                    .to_string(),
            profile_id: Some(profile.id),
            profile_name: Some(profile.name),
        });
    }

    let url = if profile.provider == "ollama" {
        build_ollama_tags_url(&profile.base_url)
    } else if profile.provider == "lm-studio-api" || profile.provider == "lm-studio" {
        let base = profile.base_url.trim_end_matches('/');
        format!("{base}/api/v1/models")
    } else if profile.provider == "openai-compatible" || profile.provider == "llama-cpp" {
        build_openai_models_url(&profile.base_url)
    } else {
        return Ok(TranslationApiConnectionStatus {
            status: "failed".to_string(),
            message: format!("不支持的 API 协议：{}", profile.provider),
            profile_id: Some(profile.id),
            profile_name: Some(profile.name),
        });
    };

    let client = build_client(5)?;
    let mut request = client.get(&url);
    if profile.provider != "ollama" {
        if let Some(api_key) = profile
            .api_key
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.bearer_auth(api_key);
        }
    }

    let profile_id = Some(profile.id.clone());
    let profile_name = Some(profile.name.clone());

    match request.send().await {
        Ok(response) if response.status().is_success() => Ok(TranslationApiConnectionStatus {
            status: "ok".to_string(),
            message: format!("{} 连接正常", profile.name),
            profile_id,
            profile_name,
        }),
        Ok(response) => {
            let status = response.status();
            let body = sanitize_message(response.text().await.unwrap_or_default());
            Ok(TranslationApiConnectionStatus {
                status: "failed".to_string(),
                message: format!("{} 连接失败：{status} {body}", profile.name),
                profile_id,
                profile_name,
            })
        }
        Err(err) => Ok(TranslationApiConnectionStatus {
            status: "failed".to_string(),
            message: format!("{} 连接失败：{}", profile.name, sanitize_message(err.to_string())),
            profile_id,
            profile_name,
        }),
    }
}

#[tauri::command]
pub async fn translate_text(
    request: TranslateTextRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, String> {
    let text = request.text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }

    let target_lang = request
        .target_lang
        .unwrap_or_else(|| "简体中文".to_string());
    let profile = get_translation_api_profile(store.inner().as_ref())?;
    let prompt = build_prompt(text, &target_lang);

    match profile.provider.as_str() {
        "ollama" => send_ollama_request(&profile, prompt, 2200).await,
        "openai-compatible" | "lm-studio-api" | "lm-studio" | "llama-cpp" => {
            send_openai_compatible_request(&profile, prompt, 2200).await
        }
        "anthropic-compatible" => Err(
            "Anthropic 兼容翻译请求还没接入，请先使用 OpenAI 兼容 / LM Studio API / llama.cpp / Ollama"
                .to_string(),
        ),
        other => Err(format!("不支持的 API 协议：{other}")),
    }
}
