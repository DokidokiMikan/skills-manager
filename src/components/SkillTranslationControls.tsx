import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  deleteSkillTranslation,
  getSkillTranslation,
  saveSkillTranslation,
  translateText,
} from "../lib/tauri";
import { hashSkillDocumentTranslationSource } from "../lib/translationHash";

export interface TranslationRenderState {
  title: string;
  description: string | null;
  content: string;
  showTranslation: boolean;
  toolbar: ReactNode;
}

interface SkillTranslationControlsProps {
  translationId: string;
  skillName: string;
  skillUpdatedAt: number;
  description?: string | null;
  content: string;
  targetLanguage?: string;
  languageCode?: string;
  disabled?: boolean;
  children: (state: TranslationRenderState) => ReactNode;
}

const TRANSLATION_CHUNK_BREAK = "---TRANSLATION_CHUNK_BREAK---";
const DEFAULT_TRANSLATION_CHUNK_MAX_CHARS = 1600;
const TRANSLATION_CHUNK_SOFT_LIMIT_RATIO = 0.75;
const TRANSLATION_CHUNK_MIN_SECTION_CHARS = 260;
const PROTECTED_PLACEHOLDER_PREFIX = "SMT";
const TRANSLATION_REVEAL_INTERVAL_MS = 65;
const TRANSLATION_REVEAL_MIN_CHARS = 24;
const TRANSLATION_REVEAL_MAX_CHARS = 72;

interface ProtectedTranslationSegment {
  placeholder: string;
  value: string;
}

interface RevealingTranslationChunk {
  index: number;
  text: string;
}

type TranslationChunkJobResult =
  | { status: "ok"; text: string }
  | { status: "error"; error: unknown };

interface TranslationChunkJob {
  index: number;
  result: Promise<TranslationChunkJobResult>;
}

const resolveTranslationLocale = (language: string | undefined) => {
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

const splitMarkdownIntoChunks = (content: string, maxChars = DEFAULT_TRANSLATION_CHUNK_MAX_CHARS) => {
  const lines = content.split(/\r?\n/);
  const chunks: string[] = [];
  let current: string[] = [];
  let inCodeBlock = false;

  const currentLength = () => current.join("\n").length;
  const pushCurrent = () => {
    const text = current.join("\n").trim();
    if (text) chunks.push(text);
    current = [];
  };

  for (const line of lines) {
    const trimmed = line.trim();
    const startsCodeFence = /^```/.test(trimmed);
    const wasInCodeBlock = inCodeBlock;
    const currentTextLength = currentLength();
    const wouldBeTooLong =
      currentTextLength + line.length + (current.length > 0 ? 1 : 0) > maxChars;
    const isHeading = /^#{1,6}\s+/.test(trimmed);
    const isBlank = trimmed === "";
    const isListItem = /^[-*+]\s+/.test(trimmed) || /^\d+\.\s+/.test(trimmed);
    const isSoftBoundary = isHeading || isBlank || isListItem;
    const isLargeEnoughSection =
      currentTextLength >= Math.max(maxChars * 0.3, TRANSLATION_CHUNK_MIN_SECTION_CHARS);
    const isPastSoftLimit = currentTextLength >= maxChars * TRANSLATION_CHUNK_SOFT_LIMIT_RATIO;

    if (
      !wasInCodeBlock &&
      current.length > 0 &&
      (wouldBeTooLong ||
        (isHeading && isLargeEnoughSection) ||
        (isSoftBoundary && isPastSoftLimit))
    ) {
      pushCurrent();
    }

    current.push(line);

    if (startsCodeFence) {
      inCodeBlock = !inCodeBlock;
    }
  }

  pushCurrent();

  return chunks.length > 0 ? chunks : content.trim() ? [content] : [];
};

const extractTranslatedHeader = (text: string) => {
  const lines = text.split(/\r?\n/);
  const titlePatterns = [/^(?:名称|标题|Name|Title)\s*[:：]\s*(.+)$/i];
  const descriptionPatterns = [/^(?:描述|简介|Description)\s*[:：]\s*(.+)$/i];

  let title: string | null = null;
  let description: string | null = null;

  for (const line of lines.slice(0, 12)) {
    const trimmed = line.trim();

    if (!title) {
      for (const pattern of titlePatterns) {
        const match = trimmed.match(pattern);
        if (match?.[1]?.trim()) {
          title = match[1].trim();
          break;
        }
      }
    }

    if (!description) {
      for (const pattern of descriptionPatterns) {
        const match = trimmed.match(pattern);
        if (match?.[1]?.trim()) {
          description = match[1].trim();
          break;
        }
      }
    }

    if (title && description) break;
  }

  return { title, description };
};

const stripTranslatedHeaderFromBody = (text: string) => {
  const lines = text.split(/\r?\n/);
  let cutIndex = 0;

  for (let index = 0; index < Math.min(lines.length, 12); index += 1) {
    const trimmed = lines[index].trim();

    if (
      /^(?:名称|标题|Name|Title)\s*[:：]/i.test(trimmed) ||
      /^(?:描述|简介|Description)\s*[:：]/i.test(trimmed)
    ) {
      cutIndex = index + 1;
      continue;
    }

    if (!trimmed && cutIndex > 0) {
      cutIndex = index + 1;
      continue;
    }

    if (cutIndex > 0) break;
  }

  return lines.slice(cutIndex).join("\n").trimStart();
};

const serializeTranslatedChunks = (chunks: string[]) =>
  chunks.join(`\n\n${TRANSLATION_CHUNK_BREAK}\n\n`);

const parseStoredTranslatedChunks = (storedContent: string) => {
  if (!storedContent.trim()) return [];

  if (storedContent.includes(TRANSLATION_CHUNK_BREAK)) {
    return storedContent
      .split(new RegExp(`\\n\\n${TRANSLATION_CHUNK_BREAK}\\n\\n`, "g"))
      .map((chunk) => chunk.trim())
      .filter(Boolean);
  }

  return [storedContent.trim()];
};

const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const protectedPlaceholderPattern = (index: number) =>
  new RegExp(
    `\\[\\[\\s*(?:${escapeRegExp(PROTECTED_PLACEHOLDER_PREFIX)}\\s*[-_\\s]*${index}|SM\\s*[-_\\s]*(?:TRANSLATION|TRANSLATIONS|TRANSLATE)?\\s*[-_\\s]*(?:PROTECTED|PROTECT|PROTECTION)?\\s*[-_\\s]*${index})\\s*\\]\\]`,
    "gi"
  );

const protectTranslationSegments = (text: string) => {
  const segments: ProtectedTranslationSegment[] = [];
  let nextText = text;

  const protect = (value: string) => {
    const placeholder = `[[${PROTECTED_PLACEHOLDER_PREFIX}${segments.length}]]`;
    segments.push({ placeholder, value });
    return placeholder;
  };

  nextText = nextText.replace(/```[\s\S]*?```/g, (match) => protect(match));
  nextText = nextText.replace(/`[^`\n]+`/g, (match) => protect(match));
  nextText = nextText.replace(
    /(^|\n)(\s*(?:\$\s+|(?:npm|pnpm|yarn|git|cargo|python|python3|node|npx|uv|pip|pip3|powershell|pwsh|cmd|curl|docker|ollama|tauri)\b)[^\n]*)/g,
    (_match, prefix: string, line: string) => `${prefix}${protect(line)}`
  );

  return { text: nextText, segments };
};

const restoreTranslationSegments = (
  text: string,
  segments: ProtectedTranslationSegment[]
) =>
  segments.reduce(
    (restored, segment, index) =>
      restored
        .split(segment.placeholder)
        .join(segment.value)
        .replace(protectedPlaceholderPattern(index), segment.value),
    text
  );

const restoreStoredTranslationSegments = (translatedChunk: string, sourceChunk?: string) => {
  if (!sourceChunk) return translatedChunk;

  const { segments } = protectTranslationSegments(sourceChunk);
  return segments.length > 0
    ? restoreTranslationSegments(translatedChunk, segments)
    : translatedChunk;
};

const hasTranslatableText = (text: string) =>
  text
    .replace(
      new RegExp(
        `\\[\\[\\s*${escapeRegExp(PROTECTED_PLACEHOLDER_PREFIX)}\\s*[-_\\s]*\\d+\\s*\\]\\]`,
        "gi"
      ),
      ""
    )
    .trim().length > 0;

const translateProtectedChunk = async (text: string, targetLanguage: string) => {
  const protectedChunk = protectTranslationSegments(text);

  if (!hasTranslatableText(protectedChunk.text)) {
    return text;
  }

  const translated = await translateText(protectedChunk.text, targetLanguage);
  return restoreTranslationSegments(translated, protectedChunk.segments);
};

const wait = (ms: number) =>
  new Promise<void>((resolve) => {
    window.setTimeout(resolve, ms);
  });

const splitPlainTextForReveal = (text: string) => {
  const frames: string[] = [];
  let buffer = "";

  const pushBuffer = () => {
    if (buffer) {
      frames.push(buffer);
      buffer = "";
    }
  };

  for (const char of text) {
    buffer += char;

    const isLineBreak = char === "\n";
    const isSentenceBreak = /[。！？!?；;:.]/.test(char);
    const shouldReveal =
      buffer.length >= TRANSLATION_REVEAL_MAX_CHARS ||
      (buffer.length >= TRANSLATION_REVEAL_MIN_CHARS && (isSentenceBreak || isLineBreak));

    if (shouldReveal) {
      pushBuffer();
    }
  }

  pushBuffer();

  return frames;
};

const splitTranslatedChunkForReveal = (text: string) => {
  const frames: string[] = [];
  const codeFencePattern = /```[\s\S]*?```/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = codeFencePattern.exec(text)) !== null) {
    frames.push(...splitPlainTextForReveal(text.slice(lastIndex, match.index)));
    frames.push(match[0]);
    lastIndex = match.index + match[0].length;
  }

  frames.push(...splitPlainTextForReveal(text.slice(lastIndex)));

  return frames.filter(Boolean);
};

export function SkillTranslationControls({
  translationId,
  skillName,
  skillUpdatedAt,
  description = null,
  content,
  targetLanguage,
  languageCode,
  disabled = false,
  children,
}: SkillTranslationControlsProps) {
  const { t, i18n } = useTranslation();
  const [translatedTitle, setTranslatedTitle] = useState<string | null>(null);
  const [translatedDescription, setTranslatedDescription] = useState<string | null>(null);
  const [translatedContent, setTranslatedContent] = useState<string | null>(null);
  const [translatedChunks, setTranslatedChunks] = useState<string[]>([]);
  const [originalChunks, setOriginalChunks] = useState<string[]>([]);
  const [showTranslation, setShowTranslation] = useState(false);
  const [translationStored, setTranslationStored] = useState(false);
  const [translationComplete, setTranslationComplete] = useState(false);
  const [translationLoading, setTranslationLoading] = useState(false);
  const [translationError, setTranslationError] = useState<string | null>(null);
  const [translationProgress, setTranslationProgress] = useState<string | null>(null);
  const [confirmRefreshOpen, setConfirmRefreshOpen] = useState(false);
  const [confirmClearOpen, setConfirmClearOpen] = useState(false);
  const [failedChunkIndex, setFailedChunkIndex] = useState<number | null>(null);
  const [activeTranslationChunkIndex, setActiveTranslationChunkIndex] = useState<number | null>(null);
  const [revealingTranslationChunk, setRevealingTranslationChunk] =
    useState<RevealingTranslationChunk | null>(null);
  const [clearingCache, setClearingCache] = useState(false);
  const [sourceHash, setSourceHash] = useState<string | null>(null);
  const translationRunIdRef = useRef(0);

  const i18nLocale = useMemo(
    () => resolveTranslationLocale(i18n.resolvedLanguage || i18n.language),
    [i18n.language, i18n.resolvedLanguage]
  );
  const effectiveTargetLanguage = targetLanguage ?? i18nLocale.targetLanguage;
  const effectiveLanguageCode = languageCode ?? i18nLocale.languageCode;
  const currentOriginalChunks = useMemo(() => splitMarkdownIntoChunks(content), [content]);

  useEffect(() => {
    let cancelled = false;

    translationRunIdRef.current += 1;
    setTranslatedTitle(null);
    setTranslatedDescription(null);
    setTranslatedContent(null);
    setTranslatedChunks([]);
    setOriginalChunks(currentOriginalChunks);
    setShowTranslation(false);
    setTranslationStored(false);
    setTranslationComplete(false);
    setTranslationError(null);
    setTranslationProgress(null);
    setTranslationLoading(false);
    setFailedChunkIndex(null);
    setActiveTranslationChunkIndex(null);
    setRevealingTranslationChunk(null);
    setSourceHash(null);

    hashSkillDocumentTranslationSource(content)
      .then((nextSourceHash) => {
        if (cancelled) return;

        setSourceHash(nextSourceHash);

        return getSkillTranslation(
          translationId,
          skillUpdatedAt,
          nextSourceHash,
          effectiveLanguageCode
        );
      })
      .then((translation) => {
        if (cancelled || translation === undefined) return;

        if (!translation) {
          setOriginalChunks(currentOriginalChunks);
          return;
        }

        const savedChunks = parseStoredTranslatedChunks(translation.content);
        const isLegacyTranslation =
          Boolean(translation.content.trim()) &&
          !translation.content.includes(TRANSLATION_CHUNK_BREAK);
        const savedSourceChunks = isLegacyTranslation ? [content] : currentOriginalChunks;
        const restoredSavedChunks = savedChunks.map((chunk, index) =>
          restoreStoredTranslationSegments(chunk, savedSourceChunks[index])
        );
        const joinedSavedContent = restoredSavedChunks.join("\n\n");
        const extractedHeader = extractTranslatedHeader(joinedSavedContent);
        const strippedContent = stripTranslatedHeaderFromBody(joinedSavedContent);

        setOriginalChunks(isLegacyTranslation ? [content] : currentOriginalChunks);
        setTranslatedChunks(restoredSavedChunks);
        setTranslatedContent(strippedContent);
        setTranslatedTitle(translation.title || extractedHeader.title);
        setTranslatedDescription(translation.description || extractedHeader.description);
        setTranslationStored(restoredSavedChunks.length > 0);
        setTranslationComplete(
          isLegacyTranslation ||
            (currentOriginalChunks.length > 0 && restoredSavedChunks.length >= currentOriginalChunks.length)
        );
      })
      .catch(() => {
        if (!cancelled) {
          setSourceHash(null);
          setTranslationStored(false);
          setTranslationComplete(false);
        }
      });

    return () => {
      cancelled = true;
      translationRunIdRef.current += 1;
    };
  }, [translationId, skillUpdatedAt, effectiveLanguageCode, content, currentOriginalChunks]);

  const translateHeader = async () => {
    const headerText = [
      `Title: ${skillName}`,
      description ? `Description: ${description}` : "",
    ]
      .filter(Boolean)
      .join("\n");

    if (!headerText.trim()) {
      return {
        title: null as string | null,
        description: null as string | null,
      };
    }

    const headerResult = await translateText(
      `Translate this skill title and description into ${effectiveTargetLanguage}. Keep this exact format:
Title: ...
Description: ...

${headerText}`,
      effectiveTargetLanguage
    );

    const titleMatch = headerResult.match(/^Title:\s*(.+)$/im);
    const descriptionMatch = headerResult.match(/^Description:\s*(.+)$/im);

    return {
      title: titleMatch?.[1]?.trim() || null,
      description: descriptionMatch?.[1]?.trim() || null,
    };
  };

  const revealTranslatedChunk = async (
    index: number,
    translatedChunk: string,
    runId: number
  ) => {
    const frames = splitTranslatedChunkForReveal(translatedChunk);

    if (frames.length === 0) {
      setRevealingTranslationChunk(null);
      return true;
    }

    let revealedText = "";
    setRevealingTranslationChunk({ index, text: "" });

    for (const frame of frames) {
      if (translationRunIdRef.current !== runId) return false;

      revealedText += frame;
      setRevealingTranslationChunk({ index, text: revealedText });

      if (frames.length > 1) {
        await wait(TRANSLATION_REVEAL_INTERVAL_MS);
      }
    }

    if (translationRunIdRef.current !== runId) return false;

    setRevealingTranslationChunk(null);
    return true;
  };

  const handleTranslate = async (options?: { refresh?: boolean }) => {
    if (!content || translationLoading || disabled) return;

    const runId = translationRunIdRef.current + 1;
    translationRunIdRef.current = runId;

    const refresh = options?.refresh === true;
    const chunks = splitMarkdownIntoChunks(content);
    const activeSourceHash =
      sourceHash ?? (await hashSkillDocumentTranslationSource(content));
    let nextChunks = refresh ? [] : [...translatedChunks];

    if (nextChunks.length > chunks.length) {
      nextChunks = [];
    }

    setOriginalChunks(chunks);
    setSourceHash(activeSourceHash);
    setTranslatedChunks(nextChunks);
    setTranslatedContent(stripTranslatedHeaderFromBody(nextChunks.join("\n\n")));
    setShowTranslation(true);
    setTranslationLoading(true);
    setTranslationError(null);
    setTranslationProgress(null);
    setFailedChunkIndex(null);
    setActiveTranslationChunkIndex(null);
    setRevealingTranslationChunk(null);
    setTranslationComplete(false);

    let activeChunkIndex: number | null = null;

    try {
      let nextTranslatedTitle = refresh ? null : translatedTitle;
      let nextTranslatedDescription = refresh ? null : translatedDescription;

      if (refresh || !nextTranslatedTitle || (description && !nextTranslatedDescription)) {
        const header = await translateHeader();

        if (translationRunIdRef.current !== runId) return;

        nextTranslatedTitle = header.title;
        nextTranslatedDescription = header.description;

        setTranslatedTitle(nextTranslatedTitle);
        setTranslatedDescription(nextTranslatedDescription);
      }

      if (chunks.length === 0) {
        await saveSkillTranslation(
          translationId,
          skillName,
          skillUpdatedAt,
          activeSourceHash,
          "",
          nextTranslatedTitle,
          nextTranslatedDescription,
          effectiveLanguageCode
        );

        if (translationRunIdRef.current !== runId) return;

        setTranslationStored(true);
        setTranslationComplete(true);
        setTranslationProgress(null);
        return;
      }

      const startTranslationJob = (index: number): TranslationChunkJob => ({
        index,
        result: translateProtectedChunk(chunks[index], effectiveTargetLanguage).then(
          (translatedChunk) => ({
            status: "ok" as const,
            text: translatedChunk.trim(),
          }),
          (error) => ({
            status: "error" as const,
            error,
          })
        ),
      });

      let pendingJob: TranslationChunkJob | null =
        nextChunks.length < chunks.length ? startTranslationJob(nextChunks.length) : null;

      while (pendingJob) {
        if (translationRunIdRef.current !== runId) return;

        const currentJob = pendingJob;
        activeChunkIndex = currentJob.index;
        setActiveTranslationChunkIndex(currentJob.index);
        setTranslationProgress(t("translation.body.progress", {
          current: currentJob.index + 1,
          total: chunks.length,
        }));

        const currentResult = await currentJob.result;

        if (translationRunIdRef.current !== runId) return;

        if (currentResult.status === "error") {
          throw currentResult.error;
        }

        pendingJob =
          currentJob.index + 1 < chunks.length
            ? startTranslationJob(currentJob.index + 1)
            : null;

        if (pendingJob) {
          setActiveTranslationChunkIndex(pendingJob.index);
          setTranslationProgress(t("translation.body.progress", {
            current: pendingJob.index + 1,
            total: chunks.length,
          }));
        } else {
          setActiveTranslationChunkIndex(null);
        }

        const translatedChunk = currentResult.text;
        setRevealingTranslationChunk({ index: currentJob.index, text: "" });

        nextChunks = [...nextChunks, translatedChunk];

        const joinedTranslatedBody = nextChunks.join("\n\n");
        const strippedTranslatedBody = stripTranslatedHeaderFromBody(joinedTranslatedBody);
        const extractedHeader = extractTranslatedHeader(joinedTranslatedBody);

        if (extractedHeader.title) {
          nextTranslatedTitle = extractedHeader.title;
          setTranslatedTitle(extractedHeader.title);
        }

        if (extractedHeader.description) {
          nextTranslatedDescription = extractedHeader.description;
          setTranslatedDescription(extractedHeader.description);
        }

        setTranslatedChunks(nextChunks);
        setTranslatedContent(strippedTranslatedBody);
        setTranslationStored(true);

        await saveSkillTranslation(
          translationId,
          skillName,
          skillUpdatedAt,
          activeSourceHash,
          serializeTranslatedChunks(nextChunks),
          nextTranslatedTitle,
          nextTranslatedDescription,
          effectiveLanguageCode
        );

        const revealCompleted = await revealTranslatedChunk(
          currentJob.index,
          translatedChunk,
          runId
        );

        if (!revealCompleted || translationRunIdRef.current !== runId) return;

        activeChunkIndex = null;
        setRevealingTranslationChunk(null);
      }

      if (translationRunIdRef.current !== runId) return;

      setTranslationStored(true);
      setTranslationComplete(true);
      setTranslationProgress(null);
    } catch (error) {
      if (translationRunIdRef.current !== runId) return;

      const message = error instanceof Error ? error.message : String(error);
      setFailedChunkIndex(activeChunkIndex);
      setActiveTranslationChunkIndex(null);
      setRevealingTranslationChunk(null);
      setTranslationProgress(
        activeChunkIndex === null
          ? null
          : t("translation.body.chunkFailed", {
              current: activeChunkIndex + 1,
              total: chunks.length,
            })
      );
      setTranslationError(message);
    } finally {
      if (translationRunIdRef.current === runId) {
        setActiveTranslationChunkIndex(null);
        setRevealingTranslationChunk(null);
        setTranslationLoading(false);
      }
    }
  };

  const handleClearTranslationCache = async () => {
    if (clearingCache || translationLoading) return;

    setClearingCache(true);
    setTranslationError(null);

    try {
      await deleteSkillTranslation(translationId, effectiveLanguageCode);
      translationRunIdRef.current += 1;
      setTranslatedTitle(null);
      setTranslatedDescription(null);
      setTranslatedContent(null);
      setTranslatedChunks([]);
      setOriginalChunks(currentOriginalChunks);
      setShowTranslation(false);
      setTranslationStored(false);
      setTranslationComplete(false);
      setFailedChunkIndex(null);
      setActiveTranslationChunkIndex(null);
      setRevealingTranslationChunk(null);
      setTranslationProgress(t("translation.body.cacheCleared"));
      setConfirmClearOpen(false);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setTranslationError(message);
    } finally {
      setClearingCache(false);
    }
  };

  const handleCancelTranslate = () => {
    translationRunIdRef.current += 1;
    setTranslationLoading(false);
    setTranslationError(null);
    setActiveTranslationChunkIndex(null);
    setRevealingTranslationChunk(null);

    if (translatedChunks.length > 0) {
      setTranslationStored(true);
      setTranslationProgress(t("translation.body.cancelledPartial"));
    } else {
      setTranslationProgress(t("translation.body.cancelled"));
    }
  };

  const effectiveOriginalChunks = originalChunks.length > 0 ? originalChunks : currentOriginalChunks;

  const mixedContent = (() => {
    if (!showTranslation) return content;

    if (!effectiveOriginalChunks.length) {
      return stripTranslatedHeaderFromBody(translatedContent || content);
    }

    const revealingChunkIndex = revealingTranslationChunk?.index ?? null;
    const translatedChunksForDisplay =
      revealingChunkIndex === null
        ? translatedChunks
        : translatedChunks.slice(0, revealingChunkIndex);
    const translatedPart = stripTranslatedHeaderFromBody(translatedChunksForDisplay.join("\n\n"));
    const revealingPart =
      revealingTranslationChunk &&
      (revealingTranslationChunk.text ||
        `> ${t("translation.body.progress", {
          current: revealingTranslationChunk.index + 1,
          total: effectiveOriginalChunks.length,
        })}`);
    const activeChunkProgress =
      !revealingPart && translationLoading && activeTranslationChunkIndex !== null
        ? `> ${t("translation.body.progress", {
            current: activeTranslationChunkIndex + 1,
            total: effectiveOriginalChunks.length,
          })}`
        : null;
    const blockedChunkIndex = revealingChunkIndex ?? activeTranslationChunkIndex;
    const remainingStartIndex =
      blockedChunkIndex === null
        ? translatedChunks.length
        : Math.max(blockedChunkIndex + 1, translatedChunks.length);
    const remainingPart = effectiveOriginalChunks.slice(remainingStartIndex).join("\n\n");

    return [translatedPart, revealingPart, activeChunkProgress, remainingPart].filter(Boolean).join("\n\n");
  })();

  const primaryButtonLabel = (() => {
    if (translationLoading) return t("translation.common.translating");

    if (translationComplete && translationStored) {
      return t("translation.common.translated");
    }

    if (translationError && failedChunkIndex !== null) {
      return t("translation.common.retryChunk");
    }

    if (translatedChunks.length > 0 && !translationComplete) {
      return t("translation.common.continue");
    }

    if (translationStored && !translationComplete) {
      return t("translation.common.continue");
    }

    return t("translation.body.translate");
  })();

  const toolbar = (
    <>
      <div className="fixed bottom-5 right-5 z-[80] flex max-w-[min(520px,calc(100vw-2rem))] flex-col items-end gap-2">
        {(translationProgress || translationError) && (
          <div
            role="status"
            aria-live="polite"
            className="max-w-full rounded-2xl border border-border-subtle bg-surface/95 px-3 py-2 text-right text-[12px] shadow-lg backdrop-blur"
          >
            {translationProgress && (
              <div className="text-muted">{translationProgress}</div>
            )}

            {translationError && (
              <div className="text-red-400">{t("translation.body.error", { error: translationError })}</div>
            )}
          </div>
        )}

        <div className="flex flex-wrap justify-end gap-2 rounded-2xl border border-border-subtle bg-surface/95 p-2 shadow-lg backdrop-blur">
          <button
            type="button"
            onClick={() => void handleTranslate()}
            disabled={translationLoading || disabled || !content || (translationComplete && translationStored)}
            className="rounded-full bg-accent px-3 py-1.5 text-[12px] font-medium text-white transition-colors hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {primaryButtonLabel}
          </button>

          {(translatedContent || translatedChunks.length > 0) && (
            <button
              type="button"
              onClick={() => setShowTranslation((prev) => !prev)}
              className="rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary"
            >
              {showTranslation ? t("translation.common.showOriginal") : t("translation.common.showTranslation")}
            </button>
          )}

          {translationLoading && (
            <button
              type="button"
              onClick={handleCancelTranslate}
              className="rounded-full bg-orange-500/10 px-3 py-1.5 text-[12px] font-medium text-orange-400 transition-colors hover:bg-orange-500/20"
            >
              {t("translation.common.cancelTranslation")}
            </button>
          )}

          {translationStored && !translationLoading && (
            <>
              <button
                type="button"
                onClick={() => setConfirmClearOpen(true)}
                disabled={disabled || clearingCache}
                className="rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("translation.common.clearCache")}
              </button>

              <button
                type="button"
                onClick={() => setConfirmRefreshOpen(true)}
                disabled={disabled || !content}
                className="rounded-full bg-red-500/10 px-3 py-1.5 text-[12px] font-medium text-red-400 transition-colors hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("translation.common.retranslate")}
              </button>
            </>
          )}
        </div>
      </div>

      {confirmRefreshOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-border-subtle bg-surface p-5 shadow-xl">
            <h3 className="text-base font-semibold text-primary">{t("translation.body.retranslateTitle")}</h3>

            <p className="mt-2 text-sm leading-6 text-secondary">
              {t("translation.body.retranslateConfirm")}
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
                  void handleTranslate({ refresh: true });
                }}
                className="rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:opacity-90"
              >
                {t("translation.common.confirmRetranslate")}
              </button>
            </div>
          </div>
        </div>
      )}

      {confirmClearOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-border-subtle bg-surface p-5 shadow-xl">
            <h3 className="text-base font-semibold text-primary">{t("translation.body.clearCacheTitle")}</h3>

            <p className="mt-2 text-sm leading-6 text-secondary">
              {t("translation.body.clearCacheConfirm")}
            </p>

            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setConfirmClearOpen(false)}
                className="rounded-xl border border-border-subtle bg-surface px-4 py-2 text-sm font-medium text-secondary transition hover:bg-surface-hover"
              >
                {t("translation.common.cancel")}
              </button>

              <button
                type="button"
                onClick={() => void handleClearTranslationCache()}
                disabled={clearingCache}
                className="rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("translation.common.confirmClearCache")}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );

  return (
    <>
      {children({
        title: showTranslation && translatedTitle ? translatedTitle : skillName,
        description:
          showTranslation && translatedDescription
            ? translatedDescription
            : description,
        content: mixedContent,
        showTranslation,
        toolbar,
      })}
    </>
  );
}
