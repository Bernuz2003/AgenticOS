export function normalizeToolExecutionText(text: string): string {
  return text.trim().startsWith("TOOL:") ? text : `[[${text}]]`;
}
