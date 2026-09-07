export function processItem(value: string) {
  const normalized = value.trim();
  const result = decrypt(normalized);
  return result.length;
}
