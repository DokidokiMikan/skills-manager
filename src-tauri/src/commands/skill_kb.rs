use std::sync::Arc;

use tauri::State;

use crate::core::{error::AppError, skill_kb, skill_store::SkillStore};

#[tauri::command]
pub async fn scan_skill_kb(
    store: State<'_, Arc<SkillStore>>,
) -> Result<skill_kb::SkillKbScanResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_kb::scan_central_skill_kb(&store).map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_kb_status() -> Result<skill_kb::SkillKbStatus, AppError> {
    tauri::async_runtime::spawn_blocking(|| skill_kb::get_skill_kb_status().map_err(AppError::io))
        .await?
}
