use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::{
    central_repo,
    skill_store::{SkillRecord, SkillStore},
};

const KB_DIR_NAME: &str = "skill-knowledge-base";
const SNAPSHOTS_DIR_NAME: &str = "snapshots";
const CHANGESETS_DIR_NAME: &str = "changesets";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const SOURCE_SCOPE_CENTRAL: &str = "central";
const MANIFEST_SCHEMA_VERSION: &str = "skill-kb-v1";
const SNAPSHOT_SCHEMA_VERSION: &str = "skill-kb-source-snapshot-v1";
const CHANGESET_SCHEMA_VERSION: &str = "skill-kb-changeset-v1";
const SKILL_DOC_CANDIDATES: &[&str] = &["SKILL.md", "skill.md"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSkillCount {
    pub active: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillKbChangeSummary {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillKbScanResult {
    pub schema_version: String,
    pub kb_version: String,
    pub generated_at: String,
    pub kb_root: String,
    pub manifest_path: String,
    pub snapshot_path: String,
    pub changeset_path: String,
    pub skill_count: ManifestSkillCount,
    pub summary: SkillKbChangeSummary,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillKbStatus {
    pub exists: bool,
    pub kb_root: String,
    pub manifest_path: String,
    pub generated_at: Option<String>,
    pub kb_version: Option<String>,
    pub skill_count: ManifestSkillCount,
    pub latest_snapshot_path: Option<String>,
    pub latest_changeset_path: Option<String>,
    pub summary: Option<SkillKbChangeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillKbManifest {
    schema_version: String,
    kb_version: String,
    generated_at: String,
    source_scope: String,
    skill_count: ManifestSkillCount,
    latest_snapshot_path: Option<String>,
    latest_changeset_path: Option<String>,
    skills: Vec<ManifestSkillEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSkillEntry {
    id: String,
    name: String,
    status: String,
    central_path: Option<String>,
    content_hash: Option<String>,
    enabled: bool,
    path_exists: bool,
    has_skill_md: bool,
    skill_md_file: Option<String>,
    last_seen_at: Option<String>,
    deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillKbSnapshot {
    schema_version: String,
    snapshot_id: String,
    kb_version: String,
    generated_at: String,
    source_scope: String,
    skill_count: ManifestSkillCount,
    skills: Vec<SkillSnapshotEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillSnapshotEntry {
    id: String,
    name: String,
    description: Option<String>,
    central_path: String,
    source_type: String,
    source_ref: Option<String>,
    source_ref_resolved: Option<String>,
    source_subpath: Option<String>,
    source_branch: Option<String>,
    source_revision: Option<String>,
    remote_revision: Option<String>,
    enabled: bool,
    created_at: i64,
    updated_at: i64,
    status: String,
    update_status: String,
    last_checked_at: Option<i64>,
    last_check_error: Option<String>,
    content_hash: Option<String>,
    path_exists: bool,
    has_skill_md: bool,
    skill_md_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillKbChangeset {
    schema_version: String,
    changeset_id: String,
    generated_at: String,
    source_scope: String,
    from_kb_version: Option<String>,
    to_kb_version: String,
    summary: SkillKbChangeSummary,
    changes: SkillKbChanges,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillKbChanges {
    added: Vec<SkillKbChangeItem>,
    updated: Vec<SkillKbChangeItem>,
    deleted: Vec<SkillKbChangeItem>,
    unchanged: Vec<SkillKbChangeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillKbChangeItem {
    id: String,
    name: String,
    central_path: Option<String>,
    previous_hash: Option<String>,
    current_hash: Option<String>,
    changed_fields: Vec<String>,
}

fn kb_root() -> PathBuf {
    central_repo::base_dir().join(KB_DIR_NAME)
}

fn manifest_path() -> PathBuf {
    kb_root().join(MANIFEST_FILE_NAME)
}

fn snapshots_dir() -> PathBuf {
    kb_root().join(SNAPSHOTS_DIR_NAME)
}

fn changesets_dir() -> PathBuf {
    kb_root().join(CHANGESETS_DIR_NAME)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn skill_doc_file(path: &Path) -> Option<String> {
    SKILL_DOC_CANDIDATES
        .iter()
        .find(|candidate| path.join(candidate).is_file())
        .map(|candidate| (*candidate).to_string())
}

fn manifest_entry_from_skill(skill: &SkillRecord, generated_at: &str) -> ManifestSkillEntry {
    let central_path = PathBuf::from(&skill.central_path);
    let path_exists = central_path.is_dir();
    let skill_md_file = if path_exists {
        skill_doc_file(&central_path)
    } else {
        None
    };
    let status = if skill.status == "deleted" {
        "deleted"
    } else {
        "active"
    };

    ManifestSkillEntry {
        id: skill.id.clone(),
        name: skill.name.clone(),
        status: status.to_string(),
        central_path: Some(skill.central_path.clone()),
        content_hash: skill.content_hash.clone(),
        enabled: skill.enabled,
        path_exists,
        has_skill_md: skill_md_file.is_some(),
        skill_md_file,
        last_seen_at: if status == "active" {
            Some(generated_at.to_string())
        } else {
            None
        },
        deleted_at: if status == "deleted" {
            Some(generated_at.to_string())
        } else {
            None
        },
    }
}

fn snapshot_entry_from_skill(skill: &SkillRecord) -> SkillSnapshotEntry {
    let central_path = PathBuf::from(&skill.central_path);
    let path_exists = central_path.is_dir();
    let skill_md_file = if path_exists {
        skill_doc_file(&central_path)
    } else {
        None
    };

    SkillSnapshotEntry {
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        central_path: skill.central_path.clone(),
        source_type: skill.source_type.clone(),
        source_ref: skill.source_ref.clone(),
        source_ref_resolved: skill.source_ref_resolved.clone(),
        source_subpath: skill.source_subpath.clone(),
        source_branch: skill.source_branch.clone(),
        source_revision: skill.source_revision.clone(),
        remote_revision: skill.remote_revision.clone(),
        enabled: skill.enabled,
        created_at: skill.created_at,
        updated_at: skill.updated_at,
        status: skill.status.clone(),
        update_status: skill.update_status.clone(),
        last_checked_at: skill.last_checked_at,
        last_check_error: skill.last_check_error.clone(),
        content_hash: skill.content_hash.clone(),
        path_exists,
        has_skill_md: skill_md_file.is_some(),
        skill_md_file,
    }
}

fn skill_count(entries: &[ManifestSkillEntry]) -> ManifestSkillCount {
    ManifestSkillCount {
        active: entries
            .iter()
            .filter(|entry| entry.status != "deleted")
            .count(),
        deleted: entries
            .iter()
            .filter(|entry| entry.status == "deleted")
            .count(),
    }
}

fn changed_fields(previous: &ManifestSkillEntry, current: &ManifestSkillEntry) -> Vec<String> {
    let mut fields = Vec::new();
    if previous.name != current.name {
        fields.push("name".to_string());
    }
    if previous.status != current.status {
        fields.push("status".to_string());
    }
    if previous.central_path != current.central_path {
        fields.push("centralPath".to_string());
    }
    if previous.content_hash != current.content_hash {
        fields.push("contentHash".to_string());
    }
    if previous.enabled != current.enabled {
        fields.push("enabled".to_string());
    }
    if previous.path_exists != current.path_exists {
        fields.push("pathExists".to_string());
    }
    if previous.has_skill_md != current.has_skill_md {
        fields.push("hasSkillMd".to_string());
    }
    if previous.skill_md_file != current.skill_md_file {
        fields.push("skillMdFile".to_string());
    }
    fields
}

fn change_item(
    entry: &ManifestSkillEntry,
    previous_hash: Option<String>,
    changed_fields: Vec<String>,
) -> SkillKbChangeItem {
    SkillKbChangeItem {
        id: entry.id.clone(),
        name: entry.name.clone(),
        central_path: entry.central_path.clone(),
        previous_hash,
        current_hash: entry.content_hash.clone(),
        changed_fields,
    }
}

fn build_changeset(
    previous_manifest: Option<&SkillKbManifest>,
    current_entries: &[ManifestSkillEntry],
    changeset_id: &str,
    generated_at: &str,
    kb_version: &str,
) -> SkillKbChangeset {
    let previous_by_id: HashMap<&str, &ManifestSkillEntry> = previous_manifest
        .map(|manifest| {
            manifest
                .skills
                .iter()
                .map(|entry| (entry.id.as_str(), entry))
                .collect()
        })
        .unwrap_or_default();
    let current_ids: HashSet<&str> = current_entries.iter().map(|entry| entry.id.as_str()).collect();

    let mut changes = SkillKbChanges::default();
    for current in current_entries {
        match previous_by_id.get(current.id.as_str()) {
            None => changes.added.push(change_item(
                current,
                None,
                vec!["created".to_string()],
            )),
            Some(previous) => {
                let fields = changed_fields(previous, current);
                if current.status == "deleted" && previous.status != "deleted" {
                    changes.deleted.push(change_item(
                        current,
                        previous.content_hash.clone(),
                        fields,
                    ));
                } else if fields.is_empty() {
                    changes.unchanged.push(change_item(
                        current,
                        previous.content_hash.clone(),
                        Vec::new(),
                    ));
                } else {
                    changes.updated.push(change_item(
                        current,
                        previous.content_hash.clone(),
                        fields,
                    ));
                }
            }
        }
    }

    for previous in previous_by_id.values() {
        if !current_ids.contains(previous.id.as_str()) {
            let mut deleted = (**previous).clone();
            deleted.status = "deleted".to_string();
            deleted.deleted_at = Some(generated_at.to_string());
            changes.deleted.push(change_item(
                &deleted,
                previous.content_hash.clone(),
                vec!["missingFromCurrentScan".to_string()],
            ));
        }
    }

    let summary = SkillKbChangeSummary {
        added: changes.added.len(),
        updated: changes.updated.len(),
        deleted: changes.deleted.len(),
        unchanged: changes.unchanged.len(),
    };

    SkillKbChangeset {
        schema_version: CHANGESET_SCHEMA_VERSION.to_string(),
        changeset_id: changeset_id.to_string(),
        generated_at: generated_at.to_string(),
        source_scope: SOURCE_SCOPE_CENTRAL.to_string(),
        from_kb_version: previous_manifest.map(|manifest| manifest.kb_version.clone()),
        to_kb_version: kb_version.to_string(),
        summary,
        changes,
    }
}

pub fn scan_central_skill_kb(store: &SkillStore) -> Result<SkillKbScanResult> {
    let now = Utc::now();
    let generated_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let scan_id = now.format("%Y%m%dT%H%M%SZ").to_string();
    let kb_version = format!("kb-{scan_id}");
    let snapshot_path = snapshots_dir().join(format!("source-{scan_id}.json"));
    let changeset_path = changesets_dir().join(format!("changeset-{scan_id}.json"));
    let manifest_path = manifest_path();

    fs::create_dir_all(snapshots_dir())?;
    fs::create_dir_all(changesets_dir())?;

    let previous_manifest = if manifest_path.exists() {
        read_json_file::<SkillKbManifest>(&manifest_path).ok()
    } else {
        None
    };

    let skills = store.get_all_skills().context("Failed to load skills")?;
    let mut manifest_entries: Vec<_> = skills
        .iter()
        .map(|skill| manifest_entry_from_skill(skill, &generated_at))
        .collect();
    manifest_entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let snapshot_entries: Vec<_> = skills.iter().map(snapshot_entry_from_skill).collect();
    let skill_count = skill_count(&manifest_entries);

    let snapshot = SkillKbSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        snapshot_id: scan_id.clone(),
        kb_version: kb_version.clone(),
        generated_at: generated_at.clone(),
        source_scope: SOURCE_SCOPE_CENTRAL.to_string(),
        skill_count: skill_count.clone(),
        skills: snapshot_entries,
    };
    write_json_file(&snapshot_path, &snapshot)?;

    let changeset = build_changeset(
        previous_manifest.as_ref(),
        &manifest_entries,
        &scan_id,
        &generated_at,
        &kb_version,
    );
    write_json_file(&changeset_path, &changeset)?;

    let manifest = SkillKbManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        kb_version: kb_version.clone(),
        generated_at: generated_at.clone(),
        source_scope: SOURCE_SCOPE_CENTRAL.to_string(),
        skill_count: skill_count.clone(),
        latest_snapshot_path: Some(snapshot_path.to_string_lossy().to_string()),
        latest_changeset_path: Some(changeset_path.to_string_lossy().to_string()),
        skills: manifest_entries,
    };
    write_json_file(&manifest_path, &manifest)?;

    Ok(SkillKbScanResult {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        kb_version,
        generated_at,
        kb_root: kb_root().to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        snapshot_path: snapshot_path.to_string_lossy().to_string(),
        changeset_path: changeset_path.to_string_lossy().to_string(),
        skill_count,
        summary: changeset.summary,
        errors: Vec::new(),
    })
}

pub fn get_skill_kb_status() -> Result<SkillKbStatus> {
    let kb_root = kb_root();
    let manifest_path = manifest_path();
    if !manifest_path.is_file() {
        return Ok(SkillKbStatus {
            exists: false,
            kb_root: kb_root.to_string_lossy().to_string(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            generated_at: None,
            kb_version: None,
            skill_count: ManifestSkillCount::default(),
            latest_snapshot_path: None,
            latest_changeset_path: None,
            summary: None,
        });
    }

    let manifest = read_json_file::<SkillKbManifest>(&manifest_path)?;
    let summary = manifest
        .latest_changeset_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .and_then(|path| read_json_file::<SkillKbChangeset>(&path).ok())
        .map(|changeset| changeset.summary);

    Ok(SkillKbStatus {
        exists: true,
        kb_root: kb_root.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        generated_at: Some(manifest.generated_at),
        kb_version: Some(manifest.kb_version),
        skill_count: manifest.skill_count,
        latest_snapshot_path: manifest.latest_snapshot_path,
        latest_changeset_path: manifest.latest_changeset_path,
        summary,
    })
}
