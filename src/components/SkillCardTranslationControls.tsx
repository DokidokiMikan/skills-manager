import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  checkTranslationApiConnection,
  deleteSkillCardTranslations,
  listSkillCardTranslations,
  saveSkillCardTranslation,
  translateText,
  type SkillCardTranslation,
} from "../lib/tauri";

export interface SkillCardTranslationItem {
  id: string;
  name: string;
  description?: string | null;
  updatedAt?: number | null;
}

interface UseSkillCardTranslationsOptions {
  disabled?: boolean;
}

const CARD_TRANSLATION_WORKER_LIMIT = 2;
const CARD_TRANSLATION_TITLE_PLACEHOLDER_PREFIX = "__SMCARD_T_";
const CARD_TRANSLATION_DESCRIPTION_PLACEHOLDER_PREFIX = "__SMCARD_D_";
const CARD_TRANSLATION_TOKEN_WHITELIST = new Set([
  "api",
  "agent",
  "anthropic",
  "ckm",
  "cli",
  "claude",
  "csv",
  "db",
  "gemma",
  "git",
  "gpt",
  "gpu",
  "http",
  "https",
  "json",
  "llama",
  "llm",
  "lm-studio",
  "md",
  "ollama",
  "openai",
  "preset",
  "py",
  "qwen",
  "sh",
  "skill",
  "sql",
  "ssh",
  "ts",
  "tsx",
  "ui",
  "uri",
  "url",
  "ux",
  "xml",
  "yaml",
  "yml",
]);
const CARD_TRANSLATION_TOKEN_PATTERN = /`[^`]+`|\b[A-Za-z0-9]+(?:[._-][A-Za-z0-9]+)*\b/g;

const resolveCardTranslationLocale = (language: string | undefined) => {
  const normalized = (language || "zh-CN").toLowerCase();

  if (
    normalized.startsWith("zh-tw") ||
    normalized.startsWith("zh-hk") ||
    normalized.includes("hant")
  ) {
    return {
      languageCode: "zh-TW",
      targetLanguage: "繁體中文",
    };
  }

  if (normalized.startsWith("en")) {
    return {
      languageCode: "en",
      targetLanguage: "English",
    };
  }

  return {
    languageCode: "zh-CN",
    targetLanguage: "简体中文",
  };
};

const hashCardText = (value: string) => {
  let hash = 0x811c9dc5;

  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }

  return (hash >>> 0).toString(16).padStart(8, "0");
};

const buildCardSourceHash = (item: SkillCardTranslationItem) => {
  const description = item.description?.trim() || "";
  return `v1:${hashCardText(`${item.name.trim()}\n${description}`)}`;
};

const identifyProtectedTokens = (text: string) => {
  const tokens = new Set<string>();
  const matches = text.match(CARD_TRANSLATION_TOKEN_PATTERN) || [];

  for (const rawToken of matches) {
    const token = rawToken.trim();
    if (!token) continue;

    const bareToken = token.startsWith("`") && token.endsWith("`")
      ? token.slice(1, -1).trim()
      : token;

    if (!bareToken) continue;

    const normalized = bareToken.toLowerCase();
    const hasPunctuation = /[._-]/.test(bareToken);
    const hasDigit = /\d/.test(bareToken);
    const isAcronym = /^[A-Z0-9]{2,}$/.test(bareToken);
    if (
      CARD_TRANSLATION_TOKEN_WHITELIST.has(normalized) ||
      isAcronym ||
      hasPunctuation ||
      hasDigit
    ) {
      tokens.add(bareToken);
    }
  }

  return Array.from(tokens).sort((a, b) => b.length - a.length || a.localeCompare(b));
};

const protectCardText = (text: string, prefix: string) => {
  const protectedTokens = identifyProtectedTokens(text);
  let protectedText = text;

  protectedTokens.forEach((token, index) => {
    protectedText = protectedText.split(token).join(`${prefix}${index}__`);
  });

  return { protectedText, protectedTokens };
};

const restoreProtectedCardText = (text: string, protectedTokens: string[], prefix: string) => {
  let restoredText = text;

  protectedTokens.forEach((token, index) => {
    restoredText = restoredText
      .split(`${prefix}${index}__`)
      .join(token);
  });

  return restoredText;
};

const parseTranslatedCard = (text: string) => {
  const cleanText = text
    .trim()
    .replace(/^```(?:json)?/i, "")
    .replace(/```$/i, "")
    .trim();

  try {
    const parsed = JSON.parse(cleanText) as {
      translatedName?: string;
      translatedDescription?: string;
      title?: string;
      description?: string;
    };

    return {
      translatedName:
        parsed.translatedName?.trim() ||
        parsed.title?.trim() ||
        "",
      translatedDescription:
        parsed.translatedDescription?.trim() ||
        parsed.description?.trim() ||
        "",
    };
  } catch {
    // Fall through to text parsing for providers that ignore JSON formatting.
  }

  const titleMatch = cleanText.match(
    /^(?:Title|标题|標題|名称|名稱|Name)\s*[:：]\s*(.+)$/im
  );
  const descriptionMatch = cleanText.match(
    /^(?:Description|描述|简介|簡介)\s*[:：]\s*([\s\S]+)$/im
  );

  if (titleMatch || descriptionMatch) {
    return {
      translatedName: titleMatch?.[1]?.trim() || "",
      translatedDescription: descriptionMatch?.[1]?.trim() || "",
    };
  }

  const lines = cleanText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  return {
    translatedName: lines[0] || "",
    translatedDescription: lines.slice(1).join(" ") || "",
  };
};

const shouldPreserveSkillCardName = (name: string) => {
  const trimmed = name.trim();

  if (!trimmed) return false;

  if (/^[A-Z0-9]{2,}$/.test(trimmed)) return true;
  if (/^[a-z0-9]{1,5}$/i.test(trimmed)) return true;

  return /^[a-z0-9]+(?:[._-][a-z0-9]+)+$/i.test(trimmed) && trimmed.length <= 16;
};

const buildTranslationPrompt = (item: SkillCardTranslationItem, targetLanguage: string) => {
  const description = item.description?.trim() || "";
  const preserveTitle = shouldPreserveSkillCardName(item.name);
  const protectedTitle = preserveTitle
    ? { protectedText: item.name, protectedTokens: [] as string[] }
    : protectCardText(item.name, CARD_TRANSLATION_TITLE_PLACEHOLDER_PREFIX);
  const protectedDescription = protectCardText(description, CARD_TRANSLATION_DESCRIPTION_PLACEHOLDER_PREFIX);
  const placeholderHint =
    protectedTitle.protectedTokens.length > 0 || protectedDescription.protectedTokens.length > 0
      ? `- Keep placeholders like ${CARD_TRANSLATION_TITLE_PLACEHOLDER_PREFIX}0__ and ${CARD_TRANSLATION_DESCRIPTION_PLACEHOLDER_PREFIX}0__ exactly unchanged.`
      : "- Keep identifiers, acronyms, product names, command names, file paths, and model names unchanged.";

  return {
    prompt: `Translate this Skill card title and description into ${targetLanguage}.
Return only compact JSON in this exact shape:
{"translatedName":"...","translatedDescription":"..."}

Rules:
- Do not add explanations.
- Keep "Skill", "Agent", "Preset", and "API" as product terms.
- Keep technical names, file paths, command names, and model names unchanged.
- Never rewrite exact identifier tokens from the source text.
${placeholderHint}
- If the description is empty, keep translatedDescription empty.

Title: ${protectedTitle.protectedText}
Description: ${protectedDescription.protectedText}`,
    preserveTitle,
    titleTokens: protectedTitle.protectedTokens,
    descriptionTokens: protectedDescription.protectedTokens,
  };
};

const isTranslationFresh = (
  item: SkillCardTranslationItem,
  translation: SkillCardTranslation | undefined
) => {
  if (!translation) return false;

  if (item.updatedAt != null && translation.skillUpdatedAt != null) {
    return item.updatedAt === translation.skillUpdatedAt;
  }

  const sourceHash = buildCardSourceHash(item);

  if (translation.sourceHash) {
    return translation.sourceHash === sourceHash;
  }

  return false;
};

export function useSkillCardTranslations(
  items: SkillCardTranslationItem[],
  options: UseSkillCardTranslationsOptions = {}
) {
  const { t, i18n } = useTranslation();
  const runIdRef = useRef(0);
  const locale = useMemo(
    () => resolveCardTranslationLocale(i18n.resolvedLanguage || i18n.language),
    [i18n.language, i18n.resolvedLanguage]
  );

  const [translations, setTranslations] = useState<Record<string, SkillCardTranslation>>({});
  const [failedItemIds, setFailedItemIds] = useState<string[]>([]);
  const [showTranslation, setShowTranslation] = useState(false);
  const [checkingApi, setCheckingApi] = useState(false);
  const [loading, setLoading] = useState(false);
  const [progressText, setProgressText] = useState<string | null>(null);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [confirmRefreshOpen, setConfirmRefreshOpen] = useState(false);

  const uniqueItems = useMemo(() => {
    const map = new Map<string, SkillCardTranslationItem>();

    for (const item of items) {
      if (!item.id || !item.name) continue;

      if (!map.has(item.id)) {
        map.set(item.id, item);
      }
    }

    return Array.from(map.values());
  }, [items]);

  const translatedCount = useMemo(
    () => uniqueItems.filter((item) => isTranslationFresh(item, translations[item.id])).length,
    [uniqueItems, translations]
  );
  const totalCount = uniqueItems.length;
  const translationComplete = totalCount > 0 && translatedCount >= totalCount;
  const retryableFailedItems = useMemo(
    () => uniqueItems.filter((item) => failedItemIds.includes(item.id)),
    [failedItemIds, uniqueItems]
  );

  useEffect(() => {
    let cancelled = false;

    setErrorText(null);
    setProgressText(null);
    setFailedItemIds([]);
    listSkillCardTranslations(locale.languageCode)
      .then((saved) => {
        if (cancelled) return;

        const next: Record<string, SkillCardTranslation> = {};

        for (const item of saved) {
          next[item.skillId] = item;
        }

        setTranslations(next);
      })
      .catch((error) => {
        if (cancelled) return;
        setErrorText(error instanceof Error ? error.message : String(error));
      });

    return () => {
      cancelled = true;
    };
  }, [locale.languageCode]);

  const translateCards = useCallback(
    async (refresh = false, targetIds?: string[]) => {
      if (loading || checkingApi || uniqueItems.length === 0 || options.disabled) return;

      const runId = runIdRef.current + 1;
      runIdRef.current = runId;

      setCheckingApi(true);
      setErrorText(null);
      setProgressText(null);
      setFailedItemIds([]);

      try {
        let nextTranslations = translations;
        const targetIdSet = targetIds && targetIds.length > 0
          ? new Set(targetIds)
          : null;
        const pendingItems = uniqueItems.filter((item) => {
          if (targetIdSet) return targetIdSet.has(item.id);
          return refresh || !isTranslationFresh(item, nextTranslations[item.id]);
        });

        if (pendingItems.length === 0) {
          setProgressText(t("translation.cards.done"));
          return;
        }

        const connection = await checkTranslationApiConnection();
        if (runIdRef.current !== runId) return;

        if (connection.status !== "ok") {
          setErrorText(connection.message);
          return;
        }

        setLoading(true);
        setShowTranslation(true);

        if (refresh) {
          const ids = uniqueItems.map((item) => item.id);
          await deleteSkillCardTranslations(ids, locale.languageCode);

          nextTranslations = { ...translations };
          for (const id of ids) {
            delete nextTranslations[id];
          }

          setTranslations(nextTranslations);
        }

        const failedIds = new Set<string>();
        let startedCount = 0;
        let cursor = 0;
        const workerCount = Math.min(CARD_TRANSLATION_WORKER_LIMIT, pendingItems.length);

        const claimNextItem = () => {
          if (cursor >= pendingItems.length) return null;
          const item = pendingItems[cursor];
          cursor += 1;
          startedCount += 1;
          setProgressText(
            t("translation.cards.progress", {
              current: startedCount,
              total: pendingItems.length,
            })
          );
          return item;
        };

        const workers = Array.from({ length: workerCount }, async () => {
          while (runIdRef.current === runId) {
            const item = claimNextItem();
            if (!item) return;

            try {
              const { prompt, preserveTitle, titleTokens, descriptionTokens } = buildTranslationPrompt(
                item,
                locale.targetLanguage
              );
              const sourceHash = buildCardSourceHash(item);

              const result = await translateText(prompt, locale.targetLanguage);

              if (runIdRef.current !== runId) return;

              const parsed = parseTranslatedCard(result);
              const translatedName = preserveTitle
                ? item.name
                : restoreProtectedCardText(
                    parsed.translatedName || item.name,
                    titleTokens,
                    CARD_TRANSLATION_TITLE_PLACEHOLDER_PREFIX
                  ) || item.name;
              const translatedDescription = restoreProtectedCardText(
                parsed.translatedDescription || item.description || "",
                descriptionTokens,
                CARD_TRANSLATION_DESCRIPTION_PLACEHOLDER_PREFIX
              ).trim();

              const saved = await saveSkillCardTranslation({
                skillId: item.id,
                skillName: item.name,
                skillUpdatedAt: item.updatedAt ?? null,
                sourceHash,
                translatedName,
                translatedDescription: translatedDescription || null,
                language: locale.languageCode,
              });

              if (runIdRef.current !== runId) return;

              setTranslations((current) => ({
                ...current,
                [item.id]: saved,
              }));
            } catch {
              if (runIdRef.current !== runId) return;
              failedIds.add(item.id);
            }
          }
        });

        await Promise.all(workers);

        if (runIdRef.current !== runId) return;

        const failedList = Array.from(failedIds);
        setFailedItemIds(failedList);
        if (failedList.length > 0) {
          setErrorText(null);
          setProgressText(t("translation.cards.partialFailed", { count: failedList.length }));
        } else {
          setErrorText(null);
          setProgressText(t("translation.cards.done"));
        }
      } catch (error) {
        if (runIdRef.current !== runId) return;

        setErrorText(error instanceof Error ? error.message : String(error));
      } finally {
        if (runIdRef.current === runId) {
          setCheckingApi(false);
          setLoading(false);
        }
      }
    },
    [
      checkingApi,
      loading,
      locale.languageCode,
      locale.targetLanguage,
      options.disabled,
      t,
      translations,
      uniqueItems,
    ]
  );

  const retryFailedCards = useCallback(() => {
    if (retryableFailedItems.length === 0) return;
    void translateCards(false, retryableFailedItems.map((item) => item.id));
  }, [retryableFailedItems, translateCards]);

  const cancelTranslation = useCallback(() => {
    runIdRef.current += 1;
    setLoading(false);
    setProgressText(
      translatedCount > 0
        ? t("translation.cards.cancelledPartial")
        : t("translation.cards.cancelled")
    );
  }, [t, translatedCount]);

  const getTranslatedCard = useCallback(
    <T extends SkillCardTranslationItem>(item: T) => {
      const translation = translations[item.id];

      if (!showTranslation || !isTranslationFresh(item, translation)) {
        return {
          name: item.name,
          description: item.description ?? null,
        };
      }

      return {
        name: shouldPreserveSkillCardName(item.name)
          ? item.name
          : translation.translatedName || item.name,
        description: translation.translatedDescription || item.description || null,
      };
    },
    [showTranslation, translations]
  );

  const getSearchText = useCallback(
    <T extends SkillCardTranslationItem>(item: T) => {
      const translation = translations[item.id];
      const parts = [item.name, item.description ?? ""];

      if (isTranslationFresh(item, translation)) {
        parts.push(translation.translatedName || "");
        parts.push(translation.translatedDescription || "");
      }

      return parts.join(" ").toLowerCase();
    },
    [translations]
  );

  const controls = (
    <>
      <div className="fixed bottom-5 right-5 z-[80] flex max-w-[min(520px,calc(100vw-2rem))] flex-col items-end gap-2">
        {(progressText || errorText) && (
          <div
            role="status"
            aria-live="polite"
            className="max-w-full rounded-2xl border border-border-subtle bg-surface/95 px-3 py-2 text-right text-[12px] shadow-lg backdrop-blur"
          >
            {progressText && <div className="text-muted">{progressText}</div>}
            {errorText && (
              <div className="text-red-400">
                {t("translation.cards.error", { error: errorText })}
              </div>
            )}
          </div>
        )}

        <div className="flex flex-wrap justify-end gap-2 rounded-2xl border border-border-subtle bg-surface/95 p-2 shadow-lg backdrop-blur">
          <button
            type="button"
            onClick={() => void translateCards(false)}
            disabled={
              checkingApi ||
              loading ||
              options.disabled ||
              totalCount === 0 ||
              (translationComplete && translatedCount > 0)
            }
            className="rounded-full bg-accent px-3 py-1.5 text-[12px] font-medium text-white transition-colors hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {checkingApi
              ? t("translation.common.checkingApi")
              : loading
              ? t("translation.common.translating")
              : translatedCount > 0 && !translationComplete
                ? t("translation.common.continue")
                : translationComplete && translatedCount > 0
                  ? t("translation.cards.translated")
                  : t("translation.cards.translate")}
          </button>

          {translatedCount > 0 && (
            <button
              type="button"
              onClick={() => setShowTranslation((prev) => !prev)}
              disabled={loading}
              className="rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary disabled:cursor-not-allowed disabled:opacity-60"
            >
              {showTranslation
                ? t("translation.common.showOriginal")
                : t("translation.common.showTranslation")}
            </button>
          )}

          {loading && (
            <button
              type="button"
              onClick={cancelTranslation}
              className="rounded-full bg-orange-500/10 px-3 py-1.5 text-[12px] font-medium text-orange-400 transition-colors hover:bg-orange-500/20"
            >
              {t("translation.common.cancelTranslation")}
            </button>
          )}

          {translatedCount > 0 && !loading && (
            <button
              type="button"
              onClick={() => setConfirmRefreshOpen(true)}
              disabled={checkingApi || options.disabled || totalCount === 0}
              className="rounded-full bg-red-500/10 px-3 py-1.5 text-[12px] font-medium text-red-400 transition-colors hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {t("translation.common.retranslate")}
            </button>
          )}

          {retryableFailedItems.length > 0 && !loading && (
            <button
              type="button"
              onClick={retryFailedCards}
              disabled={checkingApi || options.disabled}
              className="rounded-full bg-amber-500/10 px-3 py-1.5 text-[12px] font-medium text-amber-400 transition-colors hover:bg-amber-500/20 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {t("translation.cards.retryFailed")}
            </button>
          )}
        </div>
      </div>

      {confirmRefreshOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-border-subtle bg-surface p-5 shadow-xl">
            <h3 className="text-base font-semibold text-primary">
              {t("translation.cards.retranslateTitle")}
            </h3>

            <p className="mt-2 text-sm leading-6 text-secondary">
              {t("translation.cards.retranslateConfirm")}
            </p>

            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setConfirmRefreshOpen(false)}
                className="rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover"
              >
                {t("translation.common.cancel")}
              </button>

              <button
                type="button"
                onClick={() => {
                  setConfirmRefreshOpen(false);
                  void translateCards(true);
                }}
                className="rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:opacity-90"
              >
                {t("translation.common.confirmRetranslate")}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );

  return {
    controls,
    getTranslatedCard,
    getSearchText,
    showTranslation,
    translatedCount,
    totalCount,
    translationComplete,
    loading,
  };
}
