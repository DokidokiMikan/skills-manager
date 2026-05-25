# Windows junction + 预设批量同步速度优化说明

## 背景

这组改动主要解决 Windows 环境下 Skill 同步不稳定、以及预设批量应用速度偏慢的问题。

原来的 Windows 同步链路优先使用目录符号链接。普通用户权限下，Windows 经常会因为缺少创建符号链接权限而返回 `os error 1314`，导致同步必须退回到文件复制。文件复制虽然兼容性较好，但在 Skill 数量较多、目标 Agent 较多时，会带来明显的 IO 开销，也会让“已选择符号链接但实际落地为复制”的状态不够直观。

预设批量同步的旧流程也存在性能问题。前端会按 `skill × agent` 逐项调用同步命令；后端在批量添加和移除时，也有较多逐条数据库读写。以 66 个 Skill、3 个 Agent 为例，一次预设开启或关闭会涉及 198 个目标，旧流程容易出现数秒级耗时，并伴随同步后的多轮 UI / IO 刷新。

## 改动目标

- Windows junction / symlink 回退更稳。
- 预设批量应用更快，避免逐项串行调用。
- 减少预设应用后的重复刷新。
- 增加性能诊断日志，方便定位同步、数据库、刷新阶段的耗时。

## 具体改动

- `cd8af5c9 迁移 Windows junction 同步回退`
  - 将旧版本里的 Windows junction 回退能力迁移到基于作者新版的迁移分支。
  - 当目录符号链接不可用时，允许 Windows 使用 junction 作为更接近符号链接的替代路径。

- `477cdbc8 优化 Windows junction 同步性能`
  - 优化 Windows 下 junction 创建路径，减少对慢速外部命令链路的依赖。
  - 记录 Windows 符号链接不可用的状态，后续同步可更快选择 junction 回退。
  - 对已经以复制方式落地的目标，后续有机会重新升级为 symlink / junction，而不是永久停留在复制模式。

- `e01827fe 补充同步性能诊断日志`
  - 补充单项同步、批量同步、预设应用、托盘应用等关键路径的耗时日志。
  - 日志用于区分文件系统同步、数据库操作、命令整体耗时和刷新耗时。

- `fa847f00 优化预设批量同步速度`
  - 新增批量预设应用命令，让预设栏可以一次性把某个预设应用到多个 Agent。
  - 前端从逐个 `sync_skill_to_tool` / `unsync_skill_from_tool` 调用改为优先使用批量命令。
  - 显著减少 Tauri 命令往返次数。

- `6e6773d0 优化预设目标记录批量写入`
  - 后端批量添加时，先完成文件系统同步，再将 `skill_targets` 记录集中写入数据库。
  - 后端批量移除时，集中删除 `skill_targets` 记录，并保留失败时的逐条删除回退。
  - 减少 SQLite 逐条 autocommit 带来的开销。

- `144e7a45 减少预设应用后的重复刷新`
  - 预设批量应用期间临时抑制文件监听触发的全量刷新。
  - 预设完成后只刷新必要的 `managedSkills`；在单个 Agent 页面才额外刷新当前 Agent 的本地 Skill 列表。
  - 避免同步完成后重复触发 `get_managed_skills`、`get_projects`、`get_tool_status` 等调用。

## 实现效果

- Windows 环境下同步更稳定：普通权限无法创建目录符号链接时，可以更可靠地走 junction 回退。
- junction / symlink 失败时有更合理的回退路径：优先尝试更轻量的链接方案，必要时才退回复制。
- 预设批量应用速度更快：66 个 Skill 应用到 3 个 Agent 的场景，从秒级耗时降到百毫秒级。
- 减少 UI / IO 重复刷新开销：同步后不再出现多轮不必要的全量刷新。
- 日志更容易定位性能问题：可以区分文件系统、数据库、命令总耗时和刷新阶段。

## 影响范围

- Windows 文件链接逻辑，包括 symlink、junction 和 copy 回退。
- 预设批量同步逻辑，包括前端预设栏和后端批量应用命令。
- 同步后的刷新流程，包括文件监听触发的自动刷新和预设完成后的主动刷新。
- 尽量不影响普通单项同步行为；单个 Skill 的同步 / 取消同步仍保留原有命令路径。

## 验证情况

已在 Windows 环境验证：

- `cargo check --manifest-path .\src-tauri\Cargo.toml` 通过。
- `cargo test --manifest-path .\src-tauri\Cargo.toml core::scenario_service -- --nocapture` 通过。
- `cargo test --manifest-path .\src-tauri\Cargo.toml commands::presets -- --nocapture` 通过。
- `npm run build` 通过。
- 实际操作 66 个 Skill 的预设，在 3 个 Agent 上开启 / 关闭：
  - 优化前，198 个目标的批量添加约 6.9 到 8.5 秒，批量移除约 6.8 秒。
  - 数据库批量优化后，198 个目标的批量添加约 0.3 到 0.47 秒，批量移除约 0.18 秒。
  - 刷新优化后，最新一次日志显示：198 个目标批量移除约 132 ms，批量添加约 293 ms。
- 已确认 Windows 普通权限下目录 symlink 失败时会记录 `os error 1314`，并进入 junction 回退路径。

未验证：

- macOS。
- Linux。
- Windows 上的更多文件系统组合，例如网络盘、FAT/exFAT、受限企业策略环境。

## 风险与后续

- Windows 权限、开发者模式、UAC、磁盘类型和安全软件策略仍可能影响 symlink / junction 行为，需要继续观察真实用户环境。
- junction 与 symlink 在路径解析、删除行为和跨盘场景上有细微差异，后续如发现边界问题，应优先补充针对性测试。
- 文件监听抑制窗口目前用于避免预设批量应用后的重复刷新，后续需要观察是否存在极端情况下外部文件变更被短暂延后刷新。
- 如果后续向原作者提交 PR，建议拆成更小的 review 单元：
  - PR 1A：Windows symlink / junction 回退与优化。
  - PR 1B：预设批量应用性能优化。
