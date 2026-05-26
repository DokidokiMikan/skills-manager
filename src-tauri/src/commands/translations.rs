use std::sync::Arc;
use tauri::State;

use crate::core::{error::AppError, skill_store::SkillStore};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillTranslationRequest {
    pub skill_id: String,
    pub skill_updated_at: i64,
    pub source_hash: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSkillTranslationRequest {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_updated_at: i64,
    pub source_hash: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSkillTranslationRequest {
    pub skill_id: String,
    pub language: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillTranslation {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_updated_at: i64,
    pub source_hash: Option<String>,
    pub language: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<crate::core::skill_store::SkillTranslationRecord> for SkillTranslation {
    fn from(record: crate::core::skill_store::SkillTranslationRecord) -> Self {
        Self {
            skill_id: record.skill_id,
            skill_name: record.skill_name,
            skill_updated_at: record.skill_updated_at,
            source_hash: record.source_hash,
            language: record.language,
            title: record.title,
            description: record.description,
            content: record.content,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[tauri::command]
pub async fn get_skill_translation(
    request: GetSkillTranslationRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<SkillTranslation>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = request.language.unwrap_or_else(|| "zh-CN".to_string());
        store
            .get_skill_translation(
                &request.skill_id,
                request.skill_updated_at,
                request.source_hash.as_deref(),
                &language,
            )
            .map(|translation| translation.map(Into::into))
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn save_skill_translation(
    request: SaveSkillTranslationRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillTranslation, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = request.language.unwrap_or_else(|| "zh-CN".to_string());
        store
            .save_skill_translation(
                &request.skill_id,
                &request.skill_name,
                request.skill_updated_at,
                request.source_hash.as_deref(),
                &language,
                request.title.as_deref(),
                request.description.as_deref(),
                &request.content,
            )
            .map(Into::into)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn delete_skill_translation(
    request: DeleteSkillTranslationRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<usize, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = request.language.unwrap_or_else(|| "zh-CN".to_string());
        store
            .delete_skill_translation(&request.skill_id, &language)
            .map_err(AppError::db)
    })
    .await?
}
