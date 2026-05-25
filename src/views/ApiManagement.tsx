import { useEffect, useMemo, useState } from "react";
import {
  Bot,
  Cloud,
  Copy,
  Cpu,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  Plus,
  Power,
  Settings,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type { ApiProfile } from "../lib/tauri";

type ApiProvider =
  | "openai-compatible"
  | "anthropic-compatible"
  | "lm-studio-api"
  | "lm-studio"
  | "llama-cpp"
  | "ollama";

type UiApiProfile = ApiProfile & {
  modelId?: string | null;
};

type ApiStatus = "unknown" | "ok" | "failed";
type TranslationFunction = ReturnType<typeof useTranslation>["t"];

const TRANSLATION_API_PROFILE_SETTING = "translation_api_profile_id";
const TRANSLATION_API_DISABLED = "__disabled__";

const emptyForm = {
  name: "",
  provider: "openai-compatible" as ApiProvider,
  baseUrl: "",
  model: "",
  modelId: "",
  apiKey: "",
  note: "",
};

const modelLookupProviders: ApiProvider[] = [
  "openai-compatible",
  "lm-studio-api",
  "llama-cpp",
  "ollama",
  "anthropic-compatible",
];

const getProviderLabel = (provider: string, t: TranslationFunction) =>
  t(`apiManagement.providers.${provider}`, { defaultValue: provider });

const providerIcon = (provider: string) => {
  switch (provider) {
    case "ollama":
    case "llama-cpp":
      return Cpu;
    case "lm-studio-api":
    case "lm-studio":
      return Sparkles;
    case "anthropic-compatible":
      return Cloud;
    default:
      return Bot;
  }
};

const getDisplayModel = (profile: UiApiProfile, t: TranslationFunction) =>
  profile.model?.trim() ||
  profile.modelId?.trim() ||
  t("apiManagement.modelLabels.unspecified");

const getActualModelLabel = (
  profile: UiApiProfile,
  status: ApiStatus | undefined,
  t: TranslationFunction,
) => {
  const modelId = profile.modelId?.trim();
  const fallbackModel = profile.model?.trim();

  if (modelId) {
    return status === "failed"
      ? t("apiManagement.modelLabels.specifiedUnavailable", { model: modelId })
      : t("apiManagement.modelLabels.specified", { model: modelId });
  }

  if (status === "ok") {
    return fallbackModel
      ? t("apiManagement.modelLabels.autoOkWithFallback", { model: fallbackModel })
      : t("apiManagement.modelLabels.autoOk");
  }

  if (status === "failed") {
    return fallbackModel
      ? t("apiManagement.modelLabels.autoFailedWithFallback", { model: fallbackModel })
      : t("apiManagement.modelLabels.autoFailed");
  }

  return fallbackModel
    ? t("apiManagement.modelLabels.autoTryWithFallback", { model: fallbackModel })
    : t("apiManagement.modelLabels.autoTry");
};

const getErrorMessage = (error: unknown, t: TranslationFunction) => {
  const message = error instanceof Error ? error.message : String(error);

  if (message.includes("Command list_api_profiles not found")) {
    return t("apiManagement.errors.apiManagementCommandMissing");
  }
  if (message.includes("Command check_active_api_connection not found")) {
    return t("apiManagement.errors.connectionCommandMissing");
  }
  if (message.includes("Command test_api_profile not found")) {
    return t("apiManagement.errors.testCommandMissing");
  }

  return message;
};

const guessProviderFromText = (value: string): ApiProvider => {
  const lower = value.toLowerCase();

  if (lower.includes("ollama") || lower.includes("11434")) return "ollama";
  if (lower.includes("lm studio") || lower.includes("lmstudio")) return "lm-studio-api";
  if (lower.includes("anthropic") || lower.includes("claude")) return "anthropic-compatible";
  if (lower.includes("llama.cpp") || lower.includes("llamacpp") || lower.includes("llama-cpp")) {
    return "llama-cpp";
  }

  return "openai-compatible";
};

const uniqueProviders = (first: ApiProvider) =>
  [first, ...modelLookupProviders].filter(
    (provider, index, items) => items.indexOf(provider) === index,
  );

const cleanPastedValue = (value: string) =>
  value
    .trim()
    .replace(/^["'`]+|["'`，。；;,\s]+$/g, "")
    .replace(/^Bearer\s+/i, "")
    .trim();

const readLabeledValue = (line: string, labels: string[]) => {
  const escaped = labels.map((label) => label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|");
  const match = line.match(new RegExp(`^(?:${escaped})\\s*[:：=\\-]\\s*(.+)$`, "i"));
  return match ? cleanPastedValue(match[1]) : null;
};

const parseApiPasteText = (text: string, t: TranslationFunction) => {
  const lines = text
    .split(/\r?\n/)
    .map((line) => cleanPastedValue(line))
    .filter(Boolean);
  const fullText = lines.join("\n");
  const used = new Set<number>();

  let name = "";
  let baseUrl = "";
  let modelId = "";
  let apiKey = "";

  const assignLabeled = (
    setter: (value: string) => void,
    labels: string[],
    shouldUse: (value: string) => boolean = Boolean,
  ) => {
    for (const [index, line] of lines.entries()) {
      if (used.has(index)) continue;
      const value = readLabeledValue(line, labels);
      if (value && shouldUse(value)) {
        setter(value);
        used.add(index);
        return;
      }
    }
  };

  assignLabeled((value) => (name = value), [
    "name",
    "provider",
    "supplier",
    "vendor",
    "api name",
    "平台",
    "供应商",
    "供應商",
    "名称",
    "名稱",
    "名字",
  ]);
  assignLabeled((value) => (baseUrl = value), [
    "base url",
    "base_url",
    "api url",
    "endpoint",
    "url",
    "地址",
    "接口",
    "介面",
    "接口地址",
    "介面地址",
  ], (value) => /^https?:\/\//i.test(value));
  assignLabeled((value) => (modelId = value), [
    "model",
    "model id",
    "model_id",
    "deployment",
    "模型",
    "模型id",
    "模型 ID",
    "部署名",
  ]);
  assignLabeled((value) => (apiKey = value), [
    "api key",
    "apikey",
    "key",
    "token",
    "authorization",
    "密钥",
    "金鑰",
    "令牌",
    "秘钥",
  ]);

  if (!baseUrl) {
    const urlMatch = fullText.match(/https?:\/\/[^\s"'`<>，。；;,]+/i);
    if (urlMatch) baseUrl = cleanPastedValue(urlMatch[0]);
  }

  if (!apiKey) {
    const keyMatch = fullText.match(
      /\b(?:sk-[A-Za-z0-9_\-]{12,}|sk-ant-[A-Za-z0-9_\-]{12,}|AIza[A-Za-z0-9_\-]{20,}|ghu_[A-Za-z0-9_\-]{16,}|[A-Za-z0-9_\-]{32,})\b/,
    );
    if (keyMatch && !/^https?:\/\//i.test(keyMatch[0])) {
      apiKey = cleanPastedValue(keyMatch[0]);
    }
  }

  if (!modelId) {
    const modelMatch = fullText.match(
      /\b(?:gpt|o\d|claude|gemini|deepseek|qwen|llama|mistral|mixtral|glm|yi|doubao|moonshot|kimi|ernie|grok|codellama|llava)[A-Za-z0-9_.:/\-]*\b/i,
    );
    if (modelMatch) modelId = cleanPastedValue(modelMatch[0]);
  }

  if (!name) {
    const candidate = lines.find((line) => {
      if (line === baseUrl || line === modelId || line === apiKey) return false;
      if (/^https?:\/\//i.test(line)) return false;
      if (line.length > 80) return false;
      if (/(api\s*key|token|authorization|密钥|金鑰|令牌)/i.test(line)) return false;
      return true;
    });
    name = candidate || "";
  }

  return {
    name: name || t("apiManagement.defaults.unnamedApi"),
    provider: guessProviderFromText(`${name}\n${baseUrl}\n${modelId}`),
    baseUrl,
    modelId,
    apiKey,
  };
};

const emitApiConnectionStatus = (
  status: ApiStatus,
  message: string,
  profileId: string | null,
  isActive: boolean,
) => {
  window.dispatchEvent(
    new CustomEvent("api-connection-status-changed", {
      detail: { profileId, status, message, isActive },
    }),
  );
};

export function ApiManagement() {
  const { t } = useTranslation();
  const [mode, setMode] = useState<"manual" | "paste">("manual");
  const [profiles, setProfiles] = useState<UiApiProfile[]>([]);
  const [editingProfile, setEditingProfile] = useState<UiApiProfile | null>(null);
  const [manualForm, setManualForm] = useState(emptyForm);
  const [pasteText, setPasteText] = useState("");
  const [showAddForm, setShowAddForm] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showManualApiKey, setShowManualApiKey] = useState(false);
  const [showEditingApiKey, setShowEditingApiKey] = useState(false);
  const [testingProfileId, setTestingProfileId] = useState<string | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<Record<string, ApiStatus>>({});
  const [connectionMessage, setConnectionMessage] = useState<Record<string, string>>({});
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({});
  const [modelOptionsMessage, setModelOptionsMessage] = useState<Record<string, string>>({});
  const [loadingModelsFor, setLoadingModelsFor] = useState<string | null>(null);
  const [manualModelOptions, setManualModelOptions] = useState<string[]>([]);
  const [manualModelOptionsMessage, setManualModelOptionsMessage] = useState<string | null>(null);
  const [loadingManualModels, setLoadingManualModels] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<UiApiProfile | null>(null);
  const [translationProfileId, setTranslationProfileId] = useState<string | null>(null);

  const enabledProfile = useMemo(
    () => profiles.find((profile) => profile.enabled) ?? null,
    [profiles],
  );
  const translationDisabled = translationProfileId === TRANSLATION_API_DISABLED;
  const explicitTranslationProfile = useMemo(
    () =>
      translationDisabled
        ? null
        : profiles.find((profile) => profile.id === translationProfileId) ?? null,
    [profiles, translationDisabled, translationProfileId],
  );
  const translationProfile = translationDisabled
    ? null
    : explicitTranslationProfile ?? enabledProfile;
  const modelLookupHint = t("apiManagement.modelLookup.focusHint");

  const refreshProfiles = async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextProfiles, translationId] = await Promise.all([
        api.listApiProfiles() as Promise<UiApiProfile[]>,
        api.getSettings(TRANSLATION_API_PROFILE_SETTING),
      ]);
      setProfiles(nextProfiles);
      setTranslationProfileId(translationId?.trim() || null);
      await syncActiveConnection(nextProfiles);
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setLoading(false);
    }
  };

  const syncActiveConnection = async (nextProfiles: UiApiProfile[]) => {
    const activeProfile = nextProfiles.find((profile) => profile.enabled);

    if (!activeProfile) {
      setConnectionStatus({});
      setConnectionMessage({});
      emitApiConnectionStatus(
        "unknown",
        t("apiManagement.status.noEnabledApi"),
        null,
        true,
      );
      return;
    }

    setConnectionStatus((prev) => ({ ...prev, [activeProfile.id]: "unknown" }));
    setConnectionMessage((prev) => ({
      ...prev,
      [activeProfile.id]: t("apiManagement.status.checkingConnection"),
    }));

    try {
      const result = await api.checkActiveApiConnection();
      const profileId = result.profileId || activeProfile.id;
      const status = result.status === "ok" ? "ok" : "failed";
      const message =
        result.message ||
        (status === "ok"
          ? t("apiManagement.status.connectionOk")
          : t("apiManagement.status.connectionFailed"));

      setConnectionStatus((prev) => ({ ...prev, [profileId]: status }));
      setConnectionMessage((prev) => ({ ...prev, [profileId]: message }));
      emitApiConnectionStatus(status, message, profileId, true);
    } catch (err) {
      const message = getErrorMessage(err, t);
      setConnectionStatus((prev) => ({ ...prev, [activeProfile.id]: "failed" }));
      setConnectionMessage((prev) => ({ ...prev, [activeProfile.id]: message }));
      emitApiConnectionStatus("failed", message, activeProfile.id, true);
    }
  };

  useEffect(() => {
    void refreshProfiles();
  }, []);

  const handleAddManual = async () => {
    setSaving(true);
    setError(null);
    try {
      const provider = guessProviderFromText(
        `${manualForm.name}\n${manualForm.baseUrl}\n${manualForm.note}`,
      );

      await api.saveApiProfile({
        name: manualForm.name,
        provider,
        baseUrl: manualForm.baseUrl,
        model: manualForm.model || manualForm.modelId,
        modelId: manualForm.modelId || null,
        apiKey: manualForm.apiKey || null,
        enabled: profiles.length === 0,
        note: manualForm.note || null,
      });

      setManualForm(emptyForm);
      setManualModelOptions([]);
      setManualModelOptionsMessage(null);
      setShowManualApiKey(false);
      setShowAddForm(false);
      await refreshProfiles();
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const handleParsePaste = () => {
    const parsed = parseApiPasteText(pasteText, t);

    setManualForm({
      name: parsed.name,
      provider: parsed.provider,
      baseUrl: parsed.baseUrl,
      model: "",
      modelId: parsed.modelId,
      apiKey: parsed.apiKey,
      note: pasteText.trim(),
    });
    setMode("manual");
  };

  const handleSaveEditing = async () => {
    if (!editingProfile) return;

    setSaving(true);
    setError(null);
    try {
      await api.saveApiProfile({
        id: editingProfile.id,
        name: editingProfile.name,
        provider: editingProfile.provider,
        baseUrl: editingProfile.baseUrl,
        model: editingProfile.model || editingProfile.modelId || "",
        modelId: editingProfile.modelId || null,
        apiKey: editingProfile.apiKey || null,
        enabled: editingProfile.enabled,
        note: editingProfile.note || null,
      });
      setEditingProfile(null);
      setShowEditingApiKey(false);
      await refreshProfiles();
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const handleTestProfile = async (profile: UiApiProfile) => {
    setTestingProfileId(profile.id);
    setError(null);
    try {
      const result = await api.testApiProfile(profile.id);
      setConnectionStatus((prev) => ({
        ...prev,
        [profile.id]: result.ok ? "ok" : "failed",
      }));
      setConnectionMessage((prev) => ({ ...prev, [profile.id]: result.message }));
      emitApiConnectionStatus(
        result.ok ? "ok" : "failed",
        result.message,
        profile.id,
        profile.enabled,
      );
      if (!result.ok) setError(result.message);
    } catch (err) {
      const message = getErrorMessage(err, t);
      setConnectionStatus((prev) => ({ ...prev, [profile.id]: "failed" }));
      setConnectionMessage((prev) => ({ ...prev, [profile.id]: message }));
      emitApiConnectionStatus("failed", message, profile.id, profile.enabled);
      setError(message);
    } finally {
      setTestingProfileId(null);
    }
  };

  const loadModelsForConfig = async (
    form: typeof emptyForm,
    onProviderFound: (provider: ApiProvider) => void,
  ) => {
    const guessedProvider = guessProviderFromText(`${form.name}\n${form.baseUrl}\n${form.note}`);
    let lastMessage = t("apiManagement.modelLookup.failedManual");

    for (const provider of uniqueProviders(guessedProvider)) {
      const result = await api.listApiModelsForConfig({
        provider,
        baseUrl: form.baseUrl,
        apiKey: form.apiKey || null,
      });
      lastMessage = result.message;

      if (result.models.length > 0) {
        onProviderFound(provider);
        return {
          models: result.models,
          message: t("apiManagement.modelLookup.providerMessage", {
            provider: getProviderLabel(provider, t),
            message: result.message,
          }),
        };
      }
    }

    return { models: [], message: lastMessage };
  };

  const handleLoadManualModels = async () => {
    if (!manualForm.baseUrl.trim()) {
      setManualModelOptions([]);
      setManualModelOptionsMessage(t("apiManagement.modelLookup.baseUrlRequired"));
      return;
    }

    setLoadingManualModels(true);
    setManualModelOptionsMessage(t("apiManagement.modelLookup.loading"));
    try {
      const result = await loadModelsForConfig(manualForm, (provider) => {
        setManualForm((prev) => ({ ...prev, provider }));
      });
      setManualModelOptions(result.models);
      setManualModelOptionsMessage(result.message);
    } catch (err) {
      setManualModelOptions([]);
      setManualModelOptionsMessage(getErrorMessage(err, t));
    } finally {
      setLoadingManualModels(false);
    }
  };

  const handleLoadEditingModels = async (profile: UiApiProfile) => {
    if (!profile.baseUrl.trim()) {
      setModelOptions((prev) => ({ ...prev, [profile.id]: [] }));
      setModelOptionsMessage((prev) => ({
        ...prev,
        [profile.id]: t("apiManagement.modelLookup.baseUrlRequired"),
      }));
      return;
    }

    setLoadingModelsFor(profile.id);
    setModelOptionsMessage((prev) => ({
      ...prev,
      [profile.id]: t("apiManagement.modelLookup.loading"),
    }));
    try {
      const result = await loadModelsForConfig(
        {
          name: profile.name,
          provider: profile.provider as ApiProvider,
          baseUrl: profile.baseUrl,
          model: profile.model,
          modelId: profile.modelId ?? "",
          apiKey: profile.apiKey ?? "",
          note: profile.note ?? "",
        },
        (provider) => {
          setEditingProfile((prev) =>
            prev && prev.id === profile.id ? { ...prev, provider } : prev,
          );
        },
      );
      setModelOptions((prev) => ({ ...prev, [profile.id]: result.models }));
      setModelOptionsMessage((prev) => ({ ...prev, [profile.id]: result.message }));
    } catch (err) {
      const message = getErrorMessage(err, t);
      setModelOptions((prev) => ({ ...prev, [profile.id]: [] }));
      setModelOptionsMessage((prev) => ({ ...prev, [profile.id]: message }));
    } finally {
      setLoadingModelsFor(null);
    }
  };

  const toggleProfile = async (profile: UiApiProfile, enabled: boolean) => {
    setSaving(true);
    setError(null);
    try {
      const updatedProfile = (await api.setApiProfileEnabled(profile.id, enabled)) as UiApiProfile;
      setConnectionStatus((prev) => ({ ...prev, [updatedProfile.id]: "unknown" }));
      setConnectionMessage((prev) => ({
        ...prev,
        [updatedProfile.id]: enabled
          ? t("apiManagement.status.checkingConnection")
          : t("apiManagement.status.closed"),
      }));
      if (!enabled) {
        emitApiConnectionStatus(
          "unknown",
          t("apiManagement.status.apiClosed"),
          profile.id,
          profile.enabled,
        );
      }
      await refreshProfiles();

      if (enabled) {
        await handleTestProfile(updatedProfile);
      }
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const duplicateProfile = async (profile: UiApiProfile) => {
    setSaving(true);
    setError(null);
    try {
      await api.saveApiProfile({
        name: profile.name,
        provider: profile.provider,
        baseUrl: profile.baseUrl,
        model: t("apiManagement.defaults.copyName", {
          model: profile.model || profile.modelId || t("apiManagement.defaults.unnamedModel"),
        }),
        modelId: profile.modelId,
        apiKey: profile.apiKey,
        enabled: false,
        note: profile.note,
      });
      await refreshProfiles();
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const deleteProfile = async (profile: UiApiProfile) => {
    setSaving(true);
    setError(null);
    try {
      await api.deleteApiProfile(profile.id);
      if (profile.id === translationProfileId) {
        await api.setSettings(TRANSLATION_API_PROFILE_SETTING, "");
        setTranslationProfileId(null);
      }
      await refreshProfiles();
      setDeleteTarget(null);
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const toggleTranslationProfile = async (profile: UiApiProfile) => {
    setSaving(true);
    setError(null);
    try {
      const nextValue = translationProfile?.id === profile.id
        ? TRANSLATION_API_DISABLED
        : profile.id;
      await api.setSettings(TRANSLATION_API_PROFILE_SETTING, nextValue);
      setTranslationProfileId(nextValue);
    } catch (err) {
      setError(getErrorMessage(err, t));
    } finally {
      setSaving(false);
    }
  };

  const closeEditing = () => {
    setEditingProfile(null);
    setShowEditingApiKey(false);
  };

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-border-subtle bg-surface">
            <KeyRound className="h-4 w-4 text-accent" />
          </div>
          <div>
            <h1 className="text-xl font-semibold text-primary">
              {t("apiManagement.page.title")}
            </h1>
            <p className="text-[13px] text-muted">
              {t("apiManagement.page.description")}
            </p>
          </div>
        </div>

        {enabledProfile && (
          <div className="rounded-2xl border border-accent/20 bg-accent/5 px-4 py-3 text-[13px] text-secondary">
            {t("apiManagement.status.currentEnabled")}
            <span className="font-medium text-primary">{enabledProfile.name}</span>
            <span className="mx-2 text-faint">·</span>
            {enabledProfile.baseUrl}
            <span className="mx-2 text-faint">·</span>
            {getActualModelLabel(enabledProfile, connectionStatus[enabledProfile.id], t)}
          </div>
        )}

        {translationProfile && (
          <div className="rounded-2xl border border-sky-500/20 bg-sky-500/5 px-4 py-3 text-[13px] text-secondary">
            {t("apiManagement.status.translationUse")}
            <span className="font-medium text-primary">{translationProfile.name}</span>
            <span className="mx-2 text-faint">·</span>
            {translationProfile.baseUrl}
            <span className="mx-2 text-faint">·</span>
            {getDisplayModel(translationProfile, t)}
            {!explicitTranslationProfile && (
              <span className="ml-2 text-faint">
                {t("apiManagement.status.translationFollowsDefault")}
              </span>
            )}
          </div>
        )}

        <div className="flex gap-3 rounded-2xl border border-emerald-500/20 bg-emerald-500/5 px-4 py-3 text-[13px] text-secondary">
          <Cpu className="mt-0.5 h-4 w-4 shrink-0 text-emerald-400" />
          <div className="min-w-0">
            <div className="font-medium text-primary">
              {t("apiManagement.translationRecommendation.title")}
            </div>
            <p className="mt-1 leading-6 text-muted">
              {t("apiManagement.translationRecommendation.description")}
            </p>
          </div>
        </div>

        {error && (
          <div className="rounded-2xl border border-red-500/20 bg-red-500/10 px-4 py-3 text-[13px] text-red-400">
            {error}
          </div>
        )}

        {loading && (
          <div className="rounded-2xl border border-border-subtle bg-surface px-4 py-3 text-[13px] text-muted">
            {t("apiManagement.loading")}
          </div>
        )}
      </header>

      <section className="rounded-2xl border border-border-subtle bg-surface p-4 shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-base font-semibold text-primary">
              {t("apiManagement.add.title")}
            </h2>
            <p className="mt-1 text-[13px] text-muted">
              {t("apiManagement.add.description")}
            </p>
          </div>

          <button
            type="button"
            onClick={() => setShowAddForm((prev) => !prev)}
            className="inline-flex items-center gap-2 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white transition hover:opacity-90"
          >
            <Plus className="h-4 w-4" />
            {showAddForm
              ? t("apiManagement.actions.collapse")
              : t("apiManagement.actions.addModel")}
          </button>
        </div>

        {showAddForm && (
          <div className="mt-4">
            <div className="mb-4 flex w-fit rounded-full bg-bg-secondary p-1">
              <button
                type="button"
                onClick={() => setMode("manual")}
                className={cn(
                  "rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors",
                  mode === "manual"
                    ? "bg-surface text-primary shadow-sm"
                    : "text-muted hover:text-secondary",
                )}
              >
                {t("apiManagement.tabs.manual")}
              </button>
              <button
                type="button"
                onClick={() => setMode("paste")}
                className={cn(
                  "rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors",
                  mode === "paste"
                    ? "bg-surface text-primary shadow-sm"
                    : "text-muted hover:text-secondary",
                )}
              >
                {t("apiManagement.tabs.paste")}
              </button>
            </div>

            {mode === "manual" ? (
              <div className="grid gap-3 md:grid-cols-2">
                <label className="flex flex-col gap-1.5">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.apiName")}
                  </span>
                  <input
                    value={manualForm.name}
                    onChange={(event) =>
                      setManualForm((prev) => ({ ...prev, name: event.target.value }))
                    }
                    className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.placeholders.apiName")}
                  />
                </label>

                <label className="flex flex-col gap-1.5">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.modelName")}
                  </span>
                  <input
                    value={manualForm.model}
                    onChange={(event) =>
                      setManualForm((prev) => ({ ...prev, model: event.target.value }))
                    }
                    className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.placeholders.modelName")}
                  />
                </label>

                <label className="flex flex-col gap-1.5 md:col-span-2">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.baseUrl")}
                  </span>
                  <input
                    value={manualForm.baseUrl}
                    onChange={(event) =>
                      setManualForm((prev) => ({ ...prev, baseUrl: event.target.value }))
                    }
                    className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.placeholders.baseUrl")}
                  />
                </label>

                <label className="flex flex-col gap-1.5 md:col-span-2">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.apiKey")}
                  </span>
                  <div className="relative">
                    <input
                      value={manualForm.apiKey}
                      onChange={(event) =>
                        setManualForm((prev) => ({ ...prev, apiKey: event.target.value }))
                      }
                      className="w-full rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 pr-11 text-sm outline-none transition focus:border-accent"
                      placeholder={t("apiManagement.placeholders.apiKey")}
                      type={showManualApiKey ? "text" : "password"}
                    />
                    <button
                      type="button"
                      onClick={() => setShowManualApiKey((prev) => !prev)}
                      className="absolute right-2 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-lg text-muted transition hover:bg-surface-hover hover:text-secondary"
                      title={
                        showManualApiKey
                          ? t("apiManagement.actions.hideApiKey")
                          : t("apiManagement.actions.showApiKey")
                      }
                    >
                      {showManualApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                    </button>
                  </div>
                </label>

                <label className="flex flex-col gap-1.5 md:col-span-2">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.modelId")}
                  </span>
                  <input
                    value={manualForm.modelId}
                    list="manual-model-options"
                    onFocus={() => void handleLoadManualModels()}
                    onChange={(event) =>
                      setManualForm((prev) => ({ ...prev, modelId: event.target.value }))
                    }
                    className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.placeholders.modelId")}
                  />
                  <datalist id="manual-model-options">
                    {manualModelOptions.map((model) => (
                      <option key={model} value={model} />
                    ))}
                  </datalist>
                  <span className="text-[11px] text-faint">
                    {loadingManualModels
                      ? t("apiManagement.modelLookup.loading")
                      : manualModelOptionsMessage ?? modelLookupHint}
                  </span>
                </label>

                <label className="flex flex-col gap-1.5 md:col-span-2">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.fields.note")}
                  </span>
                  <textarea
                    value={manualForm.note}
                    onChange={(event) =>
                      setManualForm((prev) => ({ ...prev, note: event.target.value }))
                    }
                    className="min-h-[80px] rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.placeholders.note")}
                  />
                </label>

                <div className="flex justify-end md:col-span-2">
                  <button
                    type="button"
                    onClick={() => void handleAddManual()}
                    disabled={saving}
                    className="inline-flex items-center gap-2 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
                    {saving
                      ? t("apiManagement.actions.saving")
                      : t("apiManagement.actions.addToList")}
                  </button>
                </div>
              </div>
            ) : (
              <div className="grid gap-3">
                <label className="flex flex-col gap-1.5">
                  <span className="text-[12px] font-medium text-muted">
                    {t("apiManagement.paste.label")}
                  </span>
                  <textarea
                    value={pasteText}
                    onChange={(event) => setPasteText(event.target.value)}
                    className="min-h-[180px] rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                    placeholder={t("apiManagement.paste.placeholder")}
                  />
                </label>

                <div className="flex justify-end gap-2">
                  <button
                    type="button"
                    onClick={handleParsePaste}
                    className="inline-flex items-center gap-2 rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover"
                  >
                    <Sparkles className="h-4 w-4" />
                    {t("apiManagement.actions.parsePaste")}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </section>

      <section className="rounded-2xl border border-border-subtle bg-surface p-4 shadow-sm">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div>
            <h2 className="text-base font-semibold text-primary">
              {t("apiManagement.list.title")}
            </h2>
            <p className="mt-1 text-[13px] text-muted">
              {t("apiManagement.list.description")}
            </p>
          </div>
          <span className="rounded-full bg-bg-secondary px-2.5 py-1 text-[12px] text-muted">
            {t("apiManagement.list.count", { count: profiles.length })}
          </span>
        </div>

        {profiles.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-border-subtle bg-bg-secondary px-4 py-8 text-center text-[13px] text-muted">
            {t("apiManagement.list.empty")}
          </div>
        ) : (
          <div className="grid gap-3">
            {profiles.map((profile) => {
              const ProviderIcon = providerIcon(profile.provider);
              const status = connectionStatus[profile.id] ?? "unknown";
              const isTranslationProfile = explicitTranslationProfile
                ? profile.id === explicitTranslationProfile.id
                : !translationDisabled && profile.enabled;

              return (
                <article
                  key={profile.id}
                  className={cn(
                    "rounded-2xl border p-4 transition",
                    profile.enabled
                      ? "border-accent/30 bg-accent/5"
                      : "border-border-subtle bg-bg-secondary",
                  )}
                >
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="flex min-w-0 flex-1 gap-3">
                      <div
                        className={cn(
                          "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border",
                          profile.enabled
                            ? "border-accent/30 bg-accent/10 text-accent"
                            : "border-border-subtle bg-surface text-muted",
                        )}
                      >
                        <ProviderIcon className="h-5 w-5" />
                      </div>

                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span
                            title={connectionMessage[profile.id] || t("apiManagement.status.notChecked")}
                            className={cn(
                              "h-2.5 w-2.5 shrink-0 rounded-full",
                              status === "ok"
                                ? "bg-emerald-400"
                                : status === "failed"
                                  ? "bg-red-400"
                                  : "bg-zinc-600",
                            )}
                          />

                          <h3 className="font-semibold text-primary">
                            {getDisplayModel(profile, t)}
                          </h3>

                          <span className="rounded-full bg-surface-hover px-2 py-0.5 text-[11px] text-muted">
                            {profile.name}
                          </span>

                          <span
                            className={cn(
                              "rounded-full px-2 py-0.5 text-[11px] font-medium",
                              profile.enabled
                                ? "bg-emerald-500/10 text-emerald-400"
                                : "bg-surface-hover text-muted",
                            )}
                          >
                            {profile.enabled
                              ? t("apiManagement.status.enabled")
                              : t("apiManagement.status.disabled")}
                          </span>

                          {isTranslationProfile && (
                            <span className="rounded-full bg-sky-500/10 px-2 py-0.5 text-[11px] font-medium text-sky-400">
                              {t("apiManagement.status.translationBadge")}
                            </span>
                          )}
                        </div>

                        <div className="mt-2 flex max-w-full flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-muted">
                          <span>
                            {t("apiManagement.card.provider", {
                              provider: getProviderLabel(profile.provider, t),
                            })}
                          </span>
                          <span
                            className="min-w-0 max-w-[260px] truncate font-mono"
                            title={profile.baseUrl}
                          >
                            {t("apiManagement.card.baseUrl", { url: profile.baseUrl })}
                          </span>
                          <span
                            className={cn(
                              "min-w-0 max-w-[360px] truncate font-mono",
                              status === "failed" ? "text-amber-400" : "",
                            )}
                            title={getActualModelLabel(profile, status, t)}
                          >
                            {getActualModelLabel(profile, status, t)}
                          </span>
                          <span>
                            {t("apiManagement.card.apiKey", {
                              status: profile.apiKey
                                ? t("apiManagement.card.apiKeySet")
                                : t("apiManagement.card.apiKeyMissing"),
                            })}
                          </span>
                          {profile.note && (
                            <span
                              className="min-w-0 max-w-[260px] truncate text-faint"
                              title={profile.note}
                            >
                              {profile.note}
                            </span>
                          )}
                        </div>
                      </div>
                    </div>

                    <div className="flex shrink-0 flex-wrap justify-end gap-2">
                      <button
                        type="button"
                        onClick={() => void toggleProfile(profile, !profile.enabled)}
                        disabled={saving}
                        className={cn(
                          "inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-[12px] font-medium transition disabled:cursor-not-allowed disabled:opacity-60",
                          profile.enabled
                            ? "bg-emerald-500/10 text-emerald-400 hover:bg-emerald-500/20"
                            : "bg-surface-hover text-muted hover:text-secondary",
                        )}
                      >
                        <Power className="h-3.5 w-3.5" />
                        {profile.enabled
                          ? t("apiManagement.actions.disable")
                          : t("apiManagement.actions.enable")}
                      </button>

                      <button
                        type="button"
                        onClick={() => void handleTestProfile(profile)}
                        disabled={testingProfileId === profile.id || saving}
                        className="inline-flex items-center gap-1.5 rounded-xl bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition hover:text-secondary disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        {testingProfileId === profile.id
                          ? t("apiManagement.actions.testing")
                          : t("apiManagement.actions.test")}
                      </button>

                      <button
                        type="button"
                        onClick={() => void toggleTranslationProfile(profile)}
                        disabled={saving}
                        className={cn(
                          "inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-[12px] font-medium transition disabled:cursor-not-allowed disabled:opacity-60",
                          isTranslationProfile
                            ? "bg-sky-500/20 text-sky-300 hover:bg-sky-500/25"
                            : "bg-sky-500/10 text-sky-400 hover:bg-sky-500/20"
                        )}
                      >
                        {isTranslationProfile
                          ? t("apiManagement.actions.cancelTranslation")
                          : t("apiManagement.actions.useForTranslation")}
                      </button>

                      <button
                        type="button"
                        onClick={() => setEditingProfile(profile)}
                        className="inline-flex items-center gap-1.5 rounded-xl bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition hover:text-secondary"
                      >
                        <Settings className="h-3.5 w-3.5" />
                        {t("apiManagement.actions.settings")}
                      </button>

                      <button
                        type="button"
                        onClick={() => void duplicateProfile(profile)}
                        disabled={saving}
                        className="inline-flex items-center gap-1.5 rounded-xl bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition hover:text-secondary disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        <Copy className="h-3.5 w-3.5" />
                        {t("apiManagement.actions.copy")}
                      </button>

                      <button
                        type="button"
                        onClick={() => setDeleteTarget(profile)}
                        disabled={saving}
                        className="inline-flex items-center gap-1.5 rounded-xl bg-red-500/10 px-3 py-1.5 text-[12px] font-medium text-red-400 transition hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                        {t("apiManagement.actions.delete")}
                      </button>
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>

      {deleteTarget && (
        <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-border-subtle bg-surface p-5 shadow-xl">
            <h3 className="text-base font-semibold text-primary">
              {t("apiManagement.delete.title")}
            </h3>
            <p className="mt-2 text-sm leading-6 text-secondary">
              {t("apiManagement.delete.confirm", {
                model: getDisplayModel(deleteTarget, t),
              })}
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setDeleteTarget(null)}
                disabled={saving}
                className="rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("apiManagement.actions.cancel")}
              </button>
              <button
                type="button"
                onClick={() => void deleteProfile(deleteTarget)}
                disabled={saving}
                className="rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {saving
                  ? t("apiManagement.actions.deleting")
                  : t("apiManagement.actions.confirmDelete")}
              </button>
            </div>
          </div>
        </div>
      )}

      {editingProfile && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4"
          onClick={closeEditing}
        >
          <div
            className="w-full max-w-2xl rounded-2xl border border-border-subtle bg-surface p-5 shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="mb-4 flex items-start justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold text-primary">
                  {t("apiManagement.edit.title")}
                </h2>
                <p className="mt-1 text-[13px] text-muted">
                  {t("apiManagement.edit.description")}
                </p>
              </div>

              <button
                type="button"
                onClick={closeEditing}
                className="rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition hover:text-secondary"
              >
                {t("apiManagement.actions.close")}
              </button>
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              <label className="flex flex-col gap-1.5">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.apiName")}
                </span>
                <input
                  value={editingProfile.name}
                  onChange={(event) =>
                    setEditingProfile((prev) =>
                      prev ? { ...prev, name: event.target.value } : prev,
                    )
                  }
                  className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                />
              </label>

              <label className="flex flex-col gap-1.5">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.modelName")}
                </span>
                <input
                  value={editingProfile.model}
                  onChange={(event) =>
                    setEditingProfile((prev) =>
                      prev ? { ...prev, model: event.target.value } : prev,
                    )
                  }
                  className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                  placeholder={t("apiManagement.placeholders.modelName")}
                />
              </label>

              <label className="flex flex-col gap-1.5 md:col-span-2">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.baseUrl")}
                </span>
                <input
                  value={editingProfile.baseUrl}
                  onChange={(event) =>
                    setEditingProfile((prev) =>
                      prev ? { ...prev, baseUrl: event.target.value } : prev,
                    )
                  }
                  className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                />
              </label>

              <label className="flex flex-col gap-1.5 md:col-span-2">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.apiKey")}
                </span>
                <div className="relative">
                  <input
                    value={editingProfile.apiKey ?? ""}
                    onChange={(event) =>
                      setEditingProfile((prev) =>
                        prev ? { ...prev, apiKey: event.target.value || null } : prev,
                      )
                    }
                    type={showEditingApiKey ? "text" : "password"}
                    placeholder={t("apiManagement.placeholders.apiKey")}
                    className="w-full rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 pr-11 text-sm outline-none transition focus:border-accent"
                  />
                  <button
                    type="button"
                    onClick={() => setShowEditingApiKey((prev) => !prev)}
                    className="absolute right-2 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-lg text-muted transition hover:bg-surface-hover hover:text-secondary"
                    title={
                      showEditingApiKey
                        ? t("apiManagement.actions.hideApiKey")
                        : t("apiManagement.actions.showApiKey")
                    }
                  >
                    {showEditingApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
              </label>

              <label className="flex flex-col gap-1.5 md:col-span-2">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.modelId")}
                </span>
                <input
                  value={editingProfile.modelId ?? ""}
                  list={`models-${editingProfile.id}`}
                  onFocus={() => {
                    if (loadingModelsFor !== editingProfile.id) {
                      void handleLoadEditingModels(editingProfile);
                    }
                  }}
                  onChange={(event) =>
                    setEditingProfile((prev) =>
                      prev ? { ...prev, modelId: event.target.value || null } : prev,
                    )
                  }
                  className="rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                  placeholder={t("apiManagement.placeholders.modelId")}
                />
                <datalist id={`models-${editingProfile.id}`}>
                  {(modelOptions[editingProfile.id] ?? []).map((model) => (
                    <option key={model} value={model} />
                  ))}
                </datalist>
                <span className="text-[11px] text-faint">
                  {loadingModelsFor === editingProfile.id
                    ? t("apiManagement.modelLookup.loading")
                    : modelOptionsMessage[editingProfile.id] ?? modelLookupHint}
                </span>
              </label>

              <label className="flex flex-col gap-1.5 md:col-span-2">
                <span className="text-[12px] font-medium text-muted">
                  {t("apiManagement.fields.note")}
                </span>
                <textarea
                  value={editingProfile.note ?? ""}
                  onChange={(event) =>
                    setEditingProfile((prev) =>
                      prev ? { ...prev, note: event.target.value || null } : prev,
                    )
                  }
                  className="min-h-[90px] rounded-xl border border-border-subtle bg-bg-secondary px-3 py-2 text-sm outline-none transition focus:border-accent"
                  placeholder={t("apiManagement.placeholders.note")}
                />
              </label>
            </div>

            <div className="mt-5 flex flex-wrap justify-end gap-2">
              <button
                type="button"
                onClick={() => void handleTestProfile(editingProfile)}
                disabled={testingProfileId === editingProfile.id}
                className="rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
              >
                {testingProfileId === editingProfile.id
                  ? t("apiManagement.actions.testing")
                  : t("apiManagement.actions.testConnection")}
              </button>

              <button
                type="button"
                onClick={() =>
                  setEditingProfile((prev) => (prev ? { ...prev, enabled: true } : prev))
                }
                className="rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover"
              >
                {t("apiManagement.actions.setDefault")}
              </button>

              <button
                type="button"
                onClick={() => void handleSaveEditing()}
                disabled={saving}
                className="rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {saving
                  ? t("apiManagement.actions.saving")
                  : t("apiManagement.actions.saveSettings")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
