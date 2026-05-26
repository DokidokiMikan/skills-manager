use std::sync::Arc;
use tauri::State;

use crate::core::{error::AppError, skill_store::SkillStore};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillCardTranslationsRequest {
    pub language: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSkillCardTranslationRequest {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_updated_at: Option<i64>,
    pub source_hash: Option<String>,
    pub language: Option<String>,
    pub translated_name: String,
    pub translated_description: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSkillCardTranslationsRequest {
    pub skill_ids: Vec<String>,
    pub language: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCardTranslation {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_updated_at: Option<i64>,
    pub source_hash: Option<String>,
    pub language: String,
    pub translated_name: String,
    pub translated_description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<crate::core::skill_store::SkillCardTranslationRecord> for SkillCardTranslation {
    fn from(record: crate::core::skill_store::SkillCardTranslationRecord) -> Self {
        Self {
            skill_id: record.skill_id,
            skill_name: record.skill_name,
            skill_updated_at: record.skill_updated_at,
            source_hash: record.source_hash,
            language: record.language,
            translated_name: record.translated_name,
            translated_description: record.translated_description,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

fn language_or_default(language: Option<String>) -> String {
    language
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "zh-CN".to_string())
}

#[tauri::command]
pub async fn list_skill_card_translations(
    request: ListSkillCardTranslationsRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillCardTranslation>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = language_or_default(request.language);
        store
            .list_skill_card_translations(&language)
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn save_skill_card_translation(
    request: SaveSkillCardTranslationRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillCardTranslation, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = language_or_default(request.language);
        store
            .save_skill_card_translation(
                &request.skill_id,
                &request.skill_name,
                request.skill_updated_at,
                request.source_hash.as_deref(),
                &language,
                &request.translated_name,
                request.translated_description.as_deref(),
            )
            .map(Into::into)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn delete_skill_card_translations(
    request: DeleteSkillCardTranslationsRequest,
    store: State<'_, Arc<SkillStore>>,
) -> Result<usize, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let language = language_or_default(request.language);
        store
            .delete_skill_card_translations(&request.skill_ids, &language)
            .map_err(AppError::db)
    })
    .await?
}
