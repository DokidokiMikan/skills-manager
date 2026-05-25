const toHex = (bytes: Uint8Array) =>
  Array.from(bytes)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");

const fallbackHash = (content: string) => {
  let hash = 2166136261;

  for (let index = 0; index < content.length; index += 1) {
    hash ^= content.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }

  return `fnv1a:${(hash >>> 0).toString(16).padStart(8, "0")}`;
};

export const hashTranslationSource = async (content: string) => {
  const subtle = globalThis.crypto?.subtle;

  if (!subtle) {
    return fallbackHash(content);
  }

  const bytes = new TextEncoder().encode(content);
  const digest = await subtle.digest("SHA-256", bytes);

  return `sha256:${toHex(new Uint8Array(digest))}`;
};

export const buildSkillCardTranslationSource = (item: {
  id: string;
  name: string;
  description?: string | null;
}) =>
  [
    "skill-card:v1",
    `id:${item.id.trim()}`,
    `name:${item.name.trim()}`,
    `description:${(item.description ?? "").trim()}`,
  ].join("\n");

export const hashSkillCardTranslationSource = (item: {
  id: string;
  name: string;
  description?: string | null;
}) => hashTranslationSource(buildSkillCardTranslationSource(item));

export const hashSkillDocumentTranslationSource = (content: string) =>
  hashTranslationSource(["skill-document:v1", content].join("\n"));
