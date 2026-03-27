function stripKnownModelSuffixes(value: string): string {
  return value
    .replace(/\.(gguf|bin|safetensors)$/i, "")
    .replace(/[-_.]?(instruct|chat|base|thinking|coder|preview|it)$/i, "")
    .replace(/[-_.]?(q\d+(_k_[msl])?|iq\d+(_[msl])?|fp16|f16|bf16|8bit|4bit)$/i, "")
    .replace(/[-_.]v\d+$/i, "")
    .replace(/[-_.]+$/g, "");
}

function titleCaseWords(value: string): string {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((token) => token.charAt(0).toUpperCase() + token.slice(1))
    .join(" ");
}

function fallbackLabel(value: string): string {
  const stem = stripKnownModelSuffixes(value);
  if (!stem) {
    return value;
  }
  return titleCaseWords(stem.replace(/[._]+/g, " "));
}

function formatSizeToken(value: string): string {
  return value.includes(".") ? `${Number.parseFloat(value)}B` : `${value}B`;
}

function localModelAlias(stem: string): string | null {
  const normalized = stripKnownModelSuffixes(stem.toLowerCase().replace(/[_\s]+/g, "-"));
  const qwen = normalized.match(/qwen-?(\d+(?:\.\d+)?)?-?(\d+(?:\.\d+)?)b/);
  if (qwen) {
    const version = qwen[1] ? qwen[1] : "";
    return `Qwen${version}-${formatSizeToken(qwen[2])}`;
  }

  const llama = normalized.match(/llama-?(\d+(?:\.\d+)?)?-?(\d+(?:\.\d+)?)b/);
  if (llama) {
    const version = llama[1] ? ` ${llama[1]}` : "";
    return `Llama${version} ${formatSizeToken(llama[2])}`;
  }

  const deepseek = normalized.match(/deepseek-?([a-z0-9.]+)?-?(\d+(?:\.\d+)?)b/);
  if (deepseek) {
    const variant = deepseek[1]
      ? ` ${titleCaseWords(deepseek[1].replace(/[.-]+/g, " "))}`
      : "";
    return `DeepSeek${variant} ${formatSizeToken(deepseek[2])}`;
  }

  const gemma = normalized.match(/gemma-?(\d+(?:\.\d+)?)b/);
  if (gemma) {
    return `Gemma ${formatSizeToken(gemma[1])}`;
  }

  const mistral = normalized.match(/mistral-?(\d+(?:\.\d+)?)b/);
  if (mistral) {
    return `Mistral ${formatSizeToken(mistral[1])}`;
  }

  return null;
}

export function friendlyModelLabel(rawValue?: string | null): string {
  if (!rawValue) {
    return "n/a";
  }

  if (rawValue.includes("://")) {
    const transportParts = rawValue.split("://");
    const tail = transportParts[transportParts.length - 1] ?? rawValue;
    return fallbackLabel(tail.replace(/\//g, " "));
  }

  const pathParts = rawValue.split("/");
  const leaf = pathParts[pathParts.length - 1] ?? rawValue;
  const alias = localModelAlias(leaf) ?? localModelAlias(rawValue);
  if (alias) {
    return alias;
  }

  return fallbackLabel(leaf);
}

export function friendlyRuntimeLabel(
  runtimeLabel?: string | null,
  runtimeId?: string | null,
): string {
  if (runtimeLabel) {
    return friendlyModelLabel(runtimeLabel);
  }
  if (runtimeId) {
    return friendlyModelLabel(runtimeId);
  }
  return "unbound";
}
