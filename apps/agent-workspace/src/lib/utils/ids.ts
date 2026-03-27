export function shortId(value: string | null | undefined, length = 8): string {
  if (!value) {
    return "n/a";
  }
  return value.length <= length ? value : value.slice(0, length);
}
