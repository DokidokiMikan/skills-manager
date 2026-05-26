import { useCallback, useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  CheckCircle2,
  Database,
  FileJson,
  FolderOpen,
  Loader2,
  RefreshCcw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import { cn } from "../utils";

function formatDate(value: string | null | undefined) {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function shortPath(path: string | null | undefined) {
  if (!path) return "";
  if (path.length <= 96) return path;
  return `${path.slice(0, 46)}...${path.slice(-46)}`;
}

function SummaryTile({
  label,
  value,
  tone = "neutral",
}: {
  label: string;
  value: number | string;
  tone?: "neutral" | "good" | "warn" | "bad";
}) {
  return (
    <div
      className={cn(
        "app-panel px-4 py-3",
        tone === "good" && "border-emerald-500/30 bg-emerald-500/[0.08]",
        tone === "warn" && "border-amber-500/30 bg-amber-500/[0.08]",
        tone === "bad" && "border-red-500/30 bg-red-500/[0.08]"
      )}
    >
      <div className="text-[12px] text-muted">{label}</div>
      <div className="mt-1 text-xl font-semibold leading-none text-primary">{value}</div>
    </div>
  );
}

function PathRow({
  icon: Icon,
  label,
  path,
  disabled,
  onReveal,
}: {
  icon: typeof FolderOpen;
  label: string;
  path?: string | null;
  disabled?: boolean;
  onReveal: (path: string) => void;
}) {
  const { t } = useTranslation();
  const canReveal = Boolean(path) && !disabled;

  return (
    <div className="flex min-w-0 items-center gap-3 px-4 py-3">
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-bg-secondary text-muted">
        <Icon className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-secondary">{label}</div>
        <div className="mt-0.5 break-all text-[12px] text-muted" title={path || ""}>
          {path ? shortPath(path) : t("skillAssistant.common.none")}
        </div>
      </div>
      <button
        type="button"
        className="app-button-secondary h-8 shrink-0 px-2.5 text-[12px]"
        disabled={!canReveal}
        onClick={() => {
          if (path) onReveal(path);
        }}
      >
        <FolderOpen className="h-3.5 w-3.5" />
        {t("skillAssistant.actions.reveal")}
      </button>
    </div>
  );
}

export function SkillAssistant() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<api.SkillKbStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);

  const loadStatus = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await api.getSkillKbStatus());
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const handleScan = async () => {
    setScanning(true);
    try {
      const result = await api.scanSkillKb();
      setStatus({
        exists: true,
        kbRoot: result.kbRoot,
        manifestPath: result.manifestPath,
        generatedAt: result.generatedAt,
        kbVersion: result.kbVersion,
        skillCount: result.skillCount,
        latestSnapshotPath: result.snapshotPath,
        latestChangesetPath: result.changesetPath,
        summary: result.summary,
      });
      toast.success(t("skillAssistant.toasts.scanComplete"));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setScanning(false);
    }
  };

  const revealPath = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const generatedAt = formatDate(status?.generatedAt);
  const summary = status?.summary;

  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <div>
          <h1 className="app-page-title">{t("skillAssistant.title")}</h1>
          <p className="app-page-subtitle text-tertiary">{t("skillAssistant.subtitle")}</p>
        </div>
        <button
          type="button"
          className="app-button-primary"
          disabled={loading || scanning}
          onClick={handleScan}
        >
          {scanning ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCcw className="h-4 w-4" />
          )}
          {status?.exists
            ? t("skillAssistant.actions.refresh")
            : t("skillAssistant.actions.scan")}
        </button>
      </div>

      <div className="grid gap-3 md:grid-cols-4">
        <SummaryTile
          label={t("skillAssistant.status.label")}
          value={
            loading
              ? t("skillAssistant.status.loading")
              : status?.exists
                ? t("skillAssistant.status.ready")
                : t("skillAssistant.status.missing")
          }
          tone={status?.exists ? "good" : "warn"}
        />
        <SummaryTile
          label={t("skillAssistant.stats.activeSkills")}
          value={status?.skillCount.active ?? 0}
        />
        <SummaryTile
          label={t("skillAssistant.stats.deletedSkills")}
          value={status?.skillCount.deleted ?? 0}
        />
        <SummaryTile
          label={t("skillAssistant.stats.lastScan")}
          value={generatedAt || t("skillAssistant.common.none")}
        />
      </div>

      <div className="app-panel overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border-subtle px-4 py-3">
          <Database className="h-4 w-4 text-accent" />
          <div className="min-w-0 flex-1">
            <div className="text-[14px] font-semibold text-primary">
              {t("skillAssistant.sections.knowledgeBase")}
            </div>
            <div className="mt-0.5 truncate text-[12px] text-muted">
              {status?.kbVersion || t("skillAssistant.common.noVersion")}
            </div>
          </div>
          {status?.exists ? (
            <CheckCircle2 className="h-4 w-4 text-emerald-400" />
          ) : (
            <AlertTriangle className="h-4 w-4 text-amber-400" />
          )}
        </div>
        <div className="divide-y divide-border-subtle">
          <PathRow
            icon={FolderOpen}
            label={t("skillAssistant.paths.root")}
            path={status?.kbRoot}
            disabled={!status?.exists}
            onReveal={revealPath}
          />
          <PathRow
            icon={FileJson}
            label={t("skillAssistant.paths.manifest")}
            path={status?.manifestPath}
            disabled={!status?.exists}
            onReveal={revealPath}
          />
          <PathRow
            icon={FileJson}
            label={t("skillAssistant.paths.snapshot")}
            path={status?.latestSnapshotPath}
            disabled={!status?.exists}
            onReveal={revealPath}
          />
          <PathRow
            icon={FileJson}
            label={t("skillAssistant.paths.changeset")}
            path={status?.latestChangesetPath}
            disabled={!status?.exists}
            onReveal={revealPath}
          />
        </div>
      </div>

      {summary && (
        <div className="grid gap-3 md:grid-cols-4">
          <SummaryTile label={t("skillAssistant.changes.added")} value={summary.added} tone="good" />
          <SummaryTile label={t("skillAssistant.changes.updated")} value={summary.updated} tone="warn" />
          <SummaryTile label={t("skillAssistant.changes.deleted")} value={summary.deleted} tone="bad" />
          <SummaryTile label={t("skillAssistant.changes.unchanged")} value={summary.unchanged} />
        </div>
      )}
    </div>
  );
}
