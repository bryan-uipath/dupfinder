export function processItem(value: string) {
  const normalized = value.trim();
  const result = encrypt(normalized);
  return result.length;
}
