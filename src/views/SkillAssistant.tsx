import { useCallback, useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Archive,
  BookOpen,
  CheckCircle2,
  Database,
  FileJson,
  FileText,
  FolderOpen,
  ListChecks,
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

function StatusBadge({
  label,
  tone = "neutral",
}: {
  label: string;
  tone?: "neutral" | "good" | "warn";
}) {
  return (
    <span
      className={cn(
        "inline-flex h-6 items-center rounded-md border px-2 text-[12px] font-medium",
        tone === "good" && "border-emerald-500/30 bg-emerald-500/[0.08] text-emerald-300",
        tone === "warn" && "border-amber-500/30 bg-amber-500/[0.08] text-amber-300",
        tone === "neutral" && "border-border-subtle bg-bg-secondary text-muted"
      )}
    >
      {label}
    </span>
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

function GeneratedFileRow({ file }: { file: api.ManagedGeneratedFileStatus }) {
  const { t } = useTranslation();
  const tone = file.status === "updated" || file.previousBackedUp ? "warn" : file.status === "created" ? "good" : "neutral";

  return (
    <div className="flex min-w-0 items-center gap-3 px-4 py-2.5">
      <FileText className="h-4 w-4 shrink-0 text-muted" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium text-secondary">{file.path}</div>
        <div className="mt-0.5 truncate text-[12px] text-muted" title={file.hash}>
          {file.hash}
        </div>
      </div>
      <StatusBadge
        tone={tone}
        label={t(`skillAssistant.package.statuses.${file.status}`, {
          defaultValue: file.status,
        })}
      />
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
        assistantPackage: result.assistantPackage,
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
  const assistantPackage = status?.assistantPackage ?? null;

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

      <div className="app-panel p-4">
        <div className="mb-4 flex items-start gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-bg-secondary text-accent">
            <BookOpen className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            <div className="text-[14px] font-semibold text-primary">
              {t("skillAssistant.guide.title")}
            </div>
            <div className="mt-1 text-[13px] text-muted">
              {t("skillAssistant.guide.description")}
            </div>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-3">
          <div className="rounded-md border border-border-subtle bg-bg-secondary p-3">
            <div className="text-[13px] font-semibold text-primary">
              {t("skillAssistant.guide.whatTitle")}
            </div>
            <p className="mt-1 text-[12px] leading-5 text-muted">
              {t("skillAssistant.guide.whatBody")}
            </p>
          </div>
          <div className="rounded-md border border-border-subtle bg-bg-secondary p-3">
            <div className="text-[13px] font-semibold text-primary">
              {t("skillAssistant.guide.orderTitle")}
            </div>
            <ol className="mt-1 space-y-1 pl-4 text-[12px] leading-5 text-muted">
              <li>{t("skillAssistant.guide.order1")}</li>
              <li>{t("skillAssistant.guide.order2")}</li>
              <li>{t("skillAssistant.guide.order3")}</li>
            </ol>
          </div>
          <div className="rounded-md border border-border-subtle bg-bg-secondary p-3">
            <div className="text-[13px] font-semibold text-primary">
              {t("skillAssistant.guide.scopeTitle")}
            </div>
            <p className="mt-1 text-[12px] leading-5 text-muted">
              {t("skillAssistant.guide.scopeBody")}
            </p>
          </div>
        </div>
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

      <div className="app-panel overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border-subtle px-4 py-3">
          <ListChecks className="h-4 w-4 text-accent" />
          <div className="min-w-0 flex-1">
            <div className="text-[14px] font-semibold text-primary">
              {t("skillAssistant.package.title")}
            </div>
            <div className="mt-0.5 truncate text-[12px] text-muted">
              {assistantPackage
                ? t("skillAssistant.package.ready", { version: assistantPackage.version })
                : t("skillAssistant.package.missing")}
            </div>
          </div>
          {assistantPackage ? (
            <CheckCircle2 className="h-4 w-4 text-emerald-400" />
          ) : (
            <AlertTriangle className="h-4 w-4 text-amber-400" />
          )}
        </div>

        <div className="grid gap-3 border-b border-border-subtle p-4 md:grid-cols-4">
          <SummaryTile
            label={t("skillAssistant.package.created")}
            value={assistantPackage?.created ?? 0}
            tone={assistantPackage?.created ? "good" : "neutral"}
          />
          <SummaryTile
            label={t("skillAssistant.package.updated")}
            value={assistantPackage?.updated ?? 0}
            tone={assistantPackage?.updated ? "warn" : "neutral"}
          />
          <SummaryTile
            label={t("skillAssistant.package.unchanged")}
            value={assistantPackage?.unchanged ?? 0}
          />
          <SummaryTile
            label={t("skillAssistant.package.backedUp")}
            value={assistantPackage?.backedUp ?? 0}
            tone={assistantPackage?.backedUp ? "warn" : "neutral"}
          />
        </div>

        <div className="grid gap-3 border-b border-border-subtle p-4 md:grid-cols-4">
          <SummaryTile
            label={t("skillAssistant.enhancement.pending")}
            value={assistantPackage?.enhancement.pending ?? 0}
            tone={assistantPackage?.enhancement.pending ? "warn" : "neutral"}
          />
          <SummaryTile
            label={t("skillAssistant.enhancement.enhanced")}
            value={assistantPackage?.enhancement.enhanced ?? 0}
            tone={assistantPackage?.enhancement.enhanced ? "good" : "neutral"}
          />
          <SummaryTile
            label={t("skillAssistant.enhancement.unavailable")}
            value={assistantPackage?.enhancement.unavailable ?? 0}
            tone={assistantPackage?.enhancement.unavailable ? "bad" : "neutral"}
          />
          <SummaryTile
            label={t("skillAssistant.enhancement.total")}
            value={assistantPackage?.enhancement.total ?? 0}
          />
        </div>

        <div className="divide-y divide-border-subtle">
          <PathRow
            icon={FolderOpen}
            label={t("skillAssistant.package.output")}
            path={assistantPackage?.outputPath}
            disabled={!assistantPackage}
            onReveal={revealPath}
          />
          <PathRow
            icon={FolderOpen}
            label={t("skillAssistant.package.source")}
            path={assistantPackage?.path}
            disabled={!assistantPackage}
            onReveal={revealPath}
          />
          <PathRow
            icon={Archive}
            label={t("skillAssistant.package.zip")}
            path={assistantPackage?.zipPath}
            disabled={!assistantPackage}
            onReveal={revealPath}
          />
          <PathRow
            icon={FileJson}
            label={t("skillAssistant.package.manifest")}
            path={assistantPackage?.manifestPath}
            disabled={!assistantPackage}
            onReveal={revealPath}
          />
          <PathRow
            icon={FileJson}
            label={t("skillAssistant.enhancement.plan")}
            path={assistantPackage?.enhancementPlanPath}
            disabled={!assistantPackage}
            onReveal={revealPath}
          />
          {assistantPackage?.files.length ? (
            assistantPackage.files.map((file) => (
              <GeneratedFileRow key={file.path} file={file} />
            ))
          ) : (
            <div className="px-4 py-3 text-[12px] text-muted">
              {t("skillAssistant.package.noFiles")}
            </div>
          )}
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
