use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
const ASSISTANT_PACKAGE_SCHEMA_VERSION: &str = "skill-assistant-package-manifest-v1";
const ASSISTANT_PACKAGE_VERSION: &str = "0.1.0";
const ASSISTANT_OUTPUT_DIR_NAME: &str = "generated-assistant-skill";
const ASSISTANT_SOURCE_DIR_NAME: &str = "skill-assistant";
const ASSISTANT_PREVIOUS_DIR_NAME: &str = "_previous";
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
    pub assistant_package: SkillAssistantPackageStatus,
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
    pub assistant_package: Option<SkillAssistantPackageStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAssistantPackageStatus {
    pub output_path: String,
    pub path: String,
    pub zip_path: String,
    pub manifest_path: String,
    pub version: String,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub backed_up: usize,
    pub zip_status: String,
    pub zip_hash: String,
    pub manifest_status: String,
    pub manifest_backed_up: bool,
    pub files: Vec<ManagedGeneratedFileStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedGeneratedFileStatus {
    pub path: String,
    pub status: String,
    pub hash: String,
    pub previous_backed_up: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillAssistantPackageManifest {
    schema_version: String,
    version: String,
    generated_at: String,
    managed_by: String,
    package_name: String,
    enhancement_mode: String,
    backup_policy: String,
    previous_backup_dir: String,
    source_kb_version: String,
    active_skill_count: usize,
    deleted_skill_count: usize,
    files: Vec<ManagedGeneratedFileStatus>,
}

struct StaticGeneratedFile {
    relative_path: &'static str,
    content: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantDataManifest {
    schema_version: String,
    package_version: String,
    generated_at: String,
    source_kb_version: String,
    enhancement_mode: String,
    skill_count: ManifestSkillCount,
    files: AssistantDataFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantDataFiles {
    basic_index: String,
    cards_dir: String,
    groups_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantBasicSkillIndex {
    schema_version: String,
    generated_at: String,
    source_kb_version: String,
    enhancement_mode: String,
    skills: Vec<AssistantBasicSkillSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantBasicSkillSummary {
    id: String,
    name: String,
    description: Option<String>,
    status: String,
    enabled: bool,
    has_skill_md: bool,
    card_path: String,
    content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantSkillCard {
    schema_version: String,
    id: String,
    name: String,
    description: Option<String>,
    status: String,
    enabled: bool,
    has_skill_md: bool,
    skill_md_file: Option<String>,
    source_type: String,
    source_ref: Option<String>,
    content_hash: Option<String>,
    enhancement_status: String,
    summary: String,
    best_for: Vec<String>,
    not_for: Vec<String>,
    trigger_signals: Vec<String>,
    anti_triggers: Vec<String>,
    capabilities: Vec<String>,
    limitations: Vec<String>,
    similar_skills: Vec<String>,
    routing_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantGroupIndex {
    schema_version: String,
    generated_at: String,
    source_kb_version: String,
    groups: Vec<AssistantGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantGroup {
    id: String,
    label: String,
    skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedWriteStatus {
    Created,
    UpdatedWithBackup,
    Unchanged,
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

fn assistant_output_dir(root: &Path) -> PathBuf {
    root.join(ASSISTANT_OUTPUT_DIR_NAME)
}

fn assistant_source_dir(root: &Path) -> PathBuf {
    assistant_output_dir(root).join(ASSISTANT_SOURCE_DIR_NAME)
}

fn assistant_zip_path(root: &Path) -> PathBuf {
    assistant_output_dir(root).join(format!("{ASSISTANT_SOURCE_DIR_NAME}.zip"))
}

fn assistant_package_manifest_path(root: &Path) -> PathBuf {
    assistant_output_dir(root).join(MANIFEST_FILE_NAME)
}

fn assistant_previous_dir(root: &Path) -> PathBuf {
    assistant_output_dir(root).join(ASSISTANT_PREVIOUS_DIR_NAME)
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
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn hash_text(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn hash_bytes(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn managed_write_status_label(status: ManagedWriteStatus) -> &'static str {
    match status {
        ManagedWriteStatus::Created => "created",
        ManagedWriteStatus::UpdatedWithBackup => "updated",
        ManagedWriteStatus::Unchanged => "unchanged",
    }
}

fn write_text_with_previous_backup(
    path: &Path,
    previous_path: &Path,
    content: &str,
) -> Result<ManagedWriteStatus> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        return Ok(ManagedWriteStatus::Created);
    }

    let current =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if current == content {
        return Ok(ManagedWriteStatus::Unchanged);
    }

    if let Some(parent) = previous_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(previous_path, current)
        .with_context(|| format!("Failed to write backup {}", previous_path.display()))?;
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(ManagedWriteStatus::UpdatedWithBackup)
}

fn write_bytes_with_previous_backup(
    path: &Path,
    previous_path: &Path,
    content: &[u8],
) -> Result<ManagedWriteStatus> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        return Ok(ManagedWriteStatus::Created);
    }

    let current = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if current == content {
        return Ok(ManagedWriteStatus::Unchanged);
    }

    if let Some(parent) = previous_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(previous_path, current)
        .with_context(|| format!("Failed to write backup {}", previous_path.display()))?;
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(ManagedWriteStatus::UpdatedWithBackup)
}

fn assistant_static_files() -> Vec<StaticGeneratedFile> {
    vec![
        StaticGeneratedFile {
            relative_path: "SKILL.md",
            content: include_str!("skill_kb_templates/skill-assistant.SKILL.md"),
        },
        StaticGeneratedFile {
            relative_path: "references/answering-principles.md",
            content: include_str!("skill_kb_templates/answering-principles.md"),
        },
        StaticGeneratedFile {
            relative_path: "references/update-guide.md",
            content: include_str!("skill_kb_templates/update-guide.md"),
        },
        StaticGeneratedFile {
            relative_path: "scripts/search_skills.py",
            content: include_str!("skill_kb_templates/search_skills.py"),
        },
    ]
}

fn normalize_generated_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn sanitize_file_stem(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "skill".to_string()
    } else {
        cleaned
    }
}

fn write_generated_text_file(
    source_dir: &Path,
    previous_dir: &Path,
    relative_path: &str,
    content: &str,
) -> Result<ManagedGeneratedFileStatus> {
    let normalized_path = normalize_generated_path(relative_path);
    let target = source_dir.join(&normalized_path);
    let previous = previous_dir.join(&normalized_path);
    let write_status = write_text_with_previous_backup(&target, &previous, content)
        .with_context(|| format!("Failed to write generated assistant file {normalized_path}"))?;

    Ok(ManagedGeneratedFileStatus {
        path: normalized_path,
        status: managed_write_status_label(write_status).to_string(),
        hash: hash_text(content),
        previous_backed_up: write_status == ManagedWriteStatus::UpdatedWithBackup,
    })
}

fn write_generated_json_file<T: Serialize>(
    source_dir: &Path,
    previous_dir: &Path,
    relative_path: &str,
    value: &T,
) -> Result<ManagedGeneratedFileStatus> {
    let content = serde_json::to_string_pretty(value)?;
    write_generated_text_file(source_dir, previous_dir, relative_path, &content)
}

fn count_file_status(files: &[ManagedGeneratedFileStatus]) -> (usize, usize, usize, usize) {
    let created = files.iter().filter(|file| file.status == "created").count();
    let updated = files.iter().filter(|file| file.status == "updated").count();
    let unchanged = files.iter().filter(|file| file.status == "unchanged").count();
    let backed_up = files.iter().filter(|file| file.previous_backed_up).count();
    (created, updated, unchanged, backed_up)
}

fn write_skill_assistant_zip(
    root: &Path,
    previous_dir: &Path,
) -> Result<(ManagedWriteStatus, String)> {
    let output_dir = assistant_output_dir(root);
    let source_dir = assistant_source_dir(root);
    let zip_path = assistant_zip_path(root);
    let temp_zip_path = output_dir.join(format!("{ASSISTANT_SOURCE_DIR_NAME}.zip.tmp"));

    let mut paths: Vec<PathBuf> = WalkDir::new(&source_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    paths.sort();

    let file = fs::File::create(&temp_zip_path)
        .with_context(|| format!("Failed to create {}", temp_zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    for path in paths {
        let entry_name = path
            .strip_prefix(&output_dir)
            .with_context(|| format!("Failed to relativize {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(entry_name, opts)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let mut source_file =
            fs::File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
        let mut buffer = Vec::new();
        source_file
            .read_to_end(&mut buffer)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        zip.write_all(&buffer)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    zip.finish()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let zip_bytes = fs::read(&temp_zip_path)
        .with_context(|| format!("Failed to read {}", temp_zip_path.display()))?;
    let zip_hash = hash_bytes(&zip_bytes);
    let status = write_bytes_with_previous_backup(
        &zip_path,
        &previous_dir.join(format!("{ASSISTANT_SOURCE_DIR_NAME}.zip")),
        &zip_bytes,
    )?;
    let _ = fs::remove_file(&temp_zip_path);

    Ok((status, zip_hash))
}

fn card_path_for_skill(skill_id: &str) -> String {
    format!("data/cards/{}.json", sanitize_file_stem(skill_id))
}

fn basic_summary_for_skill(skill: &SkillRecord) -> String {
    skill.description
        .as_ref()
        .map(|description| description.trim())
        .filter(|description| !description.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Skill for tasks related to {}.", skill.name))
}

fn assistant_skill_card_from_skill(
    skill: &SkillRecord,
    entry: &ManifestSkillEntry,
) -> AssistantSkillCard {
    let summary = basic_summary_for_skill(skill);
    AssistantSkillCard {
        schema_version: "skill-assistant-card-v1".to_string(),
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        status: entry.status.clone(),
        enabled: skill.enabled,
        has_skill_md: entry.has_skill_md,
        skill_md_file: entry.skill_md_file.clone(),
        source_type: skill.source_type.clone(),
        source_ref: skill.source_ref.clone(),
        content_hash: skill.content_hash.clone(),
        enhancement_status: "basic".to_string(),
        summary: summary.clone(),
        best_for: vec![summary],
        not_for: Vec::new(),
        trigger_signals: Vec::new(),
        anti_triggers: Vec::new(),
        capabilities: Vec::new(),
        limitations: Vec::new(),
        similar_skills: Vec::new(),
        routing_notes: vec![
            "This is a basic generated card. Use semantic enhancement before relying on fine-grained routing decisions.".to_string(),
        ],
    }
}

fn source_type_groups(
    generated_at: &str,
    kb_version: &str,
    skills: &[SkillRecord],
) -> AssistantGroupIndex {
    let mut groups_by_source: HashMap<String, Vec<String>> = HashMap::new();
    for skill in skills {
        groups_by_source
            .entry(skill.source_type.clone())
            .or_default()
            .push(skill.id.clone());
    }

    let mut groups: Vec<_> = groups_by_source
        .into_iter()
        .map(|(source_type, mut skill_ids)| {
            skill_ids.sort();
            AssistantGroup {
                id: source_type.clone(),
                label: source_type,
                skill_ids,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.id.cmp(&b.id));

    AssistantGroupIndex {
        schema_version: "skill-assistant-group-index-v1".to_string(),
        generated_at: generated_at.to_string(),
        source_kb_version: kb_version.to_string(),
        groups,
    }
}

fn status_groups(
    generated_at: &str,
    kb_version: &str,
    entries: &[ManifestSkillEntry],
) -> AssistantGroupIndex {
    let mut groups_by_status: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries {
        groups_by_status
            .entry(entry.status.clone())
            .or_default()
            .push(entry.id.clone());
    }

    let mut groups: Vec<_> = groups_by_status
        .into_iter()
        .map(|(status, mut skill_ids)| {
            skill_ids.sort();
            AssistantGroup {
                id: status.clone(),
                label: status,
                skill_ids,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.id.cmp(&b.id));

    AssistantGroupIndex {
        schema_version: "skill-assistant-group-index-v1".to_string(),
        generated_at: generated_at.to_string(),
        source_kb_version: kb_version.to_string(),
        groups,
    }
}

fn ensure_skill_assistant_package(
    root: &Path,
    generated_at: &str,
    kb_version: &str,
    skill_count: &ManifestSkillCount,
    skills: &[SkillRecord],
    manifest_entries: &[ManifestSkillEntry],
) -> Result<SkillAssistantPackageStatus> {
    let output_dir = assistant_output_dir(root);
    let source_dir = assistant_source_dir(root);
    let previous_dir = assistant_previous_dir(root);
    fs::create_dir_all(&source_dir)
        .with_context(|| format!("Failed to create {}", source_dir.display()))?;
    fs::create_dir_all(&previous_dir)
        .with_context(|| format!("Failed to create {}", previous_dir.display()))?;

    let mut files = Vec::new();

    for static_file in assistant_static_files() {
        files.push(write_generated_text_file(
            &source_dir,
            &previous_dir,
            static_file.relative_path,
            static_file.content,
        )?);
    }

    let data_manifest = AssistantDataManifest {
        schema_version: "skill-assistant-data-manifest-v1".to_string(),
        package_version: ASSISTANT_PACKAGE_VERSION.to_string(),
        generated_at: generated_at.to_string(),
        source_kb_version: kb_version.to_string(),
        enhancement_mode: "basic".to_string(),
        skill_count: skill_count.clone(),
        files: AssistantDataFiles {
            basic_index: "data/index/basic-skills.json".to_string(),
            cards_dir: "data/cards/".to_string(),
            groups_dir: "data/groups/".to_string(),
        },
    };
    files.push(write_generated_json_file(
        &source_dir,
        &previous_dir,
        "data/manifest.json",
        &data_manifest,
    )?);

    let entry_by_id: HashMap<&str, &ManifestSkillEntry> = manifest_entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let mut summaries = Vec::new();
    let mut sorted_skills = skills.to_vec();
    sorted_skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    for skill in &sorted_skills {
        let Some(entry) = entry_by_id.get(skill.id.as_str()) else {
            continue;
        };
        let card_path = card_path_for_skill(&skill.id);
        let card = assistant_skill_card_from_skill(skill, entry);
        files.push(write_generated_json_file(
            &source_dir,
            &previous_dir,
            &card_path,
            &card,
        )?);

        summaries.push(AssistantBasicSkillSummary {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            status: entry.status.clone(),
            enabled: skill.enabled,
            has_skill_md: entry.has_skill_md,
            card_path,
            content_hash: skill.content_hash.clone(),
        });
    }

    let basic_index = AssistantBasicSkillIndex {
        schema_version: "skill-assistant-basic-index-v1".to_string(),
        generated_at: generated_at.to_string(),
        source_kb_version: kb_version.to_string(),
        enhancement_mode: "basic".to_string(),
        skills: summaries,
    };
    files.push(write_generated_json_file(
        &source_dir,
        &previous_dir,
        "data/index/basic-skills.json",
        &basic_index,
    )?);

    files.push(write_generated_json_file(
        &source_dir,
        &previous_dir,
        "data/groups/source-types.json",
        &source_type_groups(generated_at, kb_version, skills),
    )?);
    files.push(write_generated_json_file(
        &source_dir,
        &previous_dir,
        "data/groups/status.json",
        &status_groups(generated_at, kb_version, manifest_entries),
    )?);

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let package_manifest = SkillAssistantPackageManifest {
        schema_version: ASSISTANT_PACKAGE_SCHEMA_VERSION.to_string(),
        version: ASSISTANT_PACKAGE_VERSION.to_string(),
        generated_at: generated_at.to_string(),
        managed_by: "SkillManager".to_string(),
        package_name: ASSISTANT_SOURCE_DIR_NAME.to_string(),
        enhancement_mode: "basic".to_string(),
        backup_policy: "keep_previous_version_only".to_string(),
        previous_backup_dir: ASSISTANT_PREVIOUS_DIR_NAME.to_string(),
        source_kb_version: kb_version.to_string(),
        active_skill_count: skill_count.active,
        deleted_skill_count: skill_count.deleted,
        files: files.clone(),
    };
    let package_manifest_text = serde_json::to_string_pretty(&package_manifest)?;
    let package_manifest_path = assistant_package_manifest_path(root);
    let package_manifest_previous_path = previous_dir.join(MANIFEST_FILE_NAME);
    let package_manifest_write_status = write_text_with_previous_backup(
        &package_manifest_path,
        &package_manifest_previous_path,
        &package_manifest_text,
    )
    .context("Failed to write skill assistant package manifest")?;
    let (zip_status, zip_hash) = write_skill_assistant_zip(root, &previous_dir)?;

    let (created, updated, unchanged, backed_up) = count_file_status(&files);
    Ok(SkillAssistantPackageStatus {
        output_path: output_dir.to_string_lossy().to_string(),
        path: source_dir.to_string_lossy().to_string(),
        zip_path: assistant_zip_path(root).to_string_lossy().to_string(),
        manifest_path: package_manifest_path.to_string_lossy().to_string(),
        version: ASSISTANT_PACKAGE_VERSION.to_string(),
        created,
        updated,
        unchanged,
        backed_up,
        zip_status: managed_write_status_label(zip_status).to_string(),
        zip_hash,
        manifest_status: managed_write_status_label(package_manifest_write_status).to_string(),
        manifest_backed_up: package_manifest_write_status == ManagedWriteStatus::UpdatedWithBackup,
        files,
    })
}

fn get_skill_assistant_package_status(
    root: &Path,
) -> Result<Option<SkillAssistantPackageStatus>> {
    let manifest_path = assistant_package_manifest_path(root);
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest = read_json_file::<SkillAssistantPackageManifest>(&manifest_path)?;
    let unchanged = manifest.files.len();
    Ok(Some(SkillAssistantPackageStatus {
        output_path: assistant_output_dir(root).to_string_lossy().to_string(),
        path: assistant_source_dir(root).to_string_lossy().to_string(),
        zip_path: assistant_zip_path(root).to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        version: manifest.version,
        created: 0,
        updated: 0,
        unchanged,
        backed_up: 0,
        zip_status: "present".to_string(),
        zip_hash: fs::read(assistant_zip_path(root))
            .map(|bytes| hash_bytes(&bytes))
            .unwrap_or_default(),
        manifest_status: "present".to_string(),
        manifest_backed_up: false,
        files: manifest.files,
    }))
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
    let current_ids: HashSet<&str> = current_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();

    let mut changes = SkillKbChanges::default();
    for current in current_entries {
        match previous_by_id.get(current.id.as_str()) {
            None => changes
                .added
                .push(change_item(current, None, vec!["created".to_string()])),
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
    let kb_root = kb_root();
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
        skills: manifest_entries.clone(),
    };
    write_json_file(&manifest_path, &manifest)?;
    let assistant_package = ensure_skill_assistant_package(
        &kb_root,
        &generated_at,
        &kb_version,
        &skill_count,
        &skills,
        &manifest_entries,
    )?;

    Ok(SkillKbScanResult {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        kb_version,
        generated_at,
        kb_root: kb_root.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        snapshot_path: snapshot_path.to_string_lossy().to_string(),
        changeset_path: changeset_path.to_string_lossy().to_string(),
        skill_count,
        summary: changeset.summary,
        errors: Vec::new(),
        assistant_package,
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
            assistant_package: None,
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
        assistant_package: get_skill_assistant_package_status(&kb_root)?,
    })
}
