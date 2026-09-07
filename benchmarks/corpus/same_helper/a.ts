export function uniqueEdge(values: string[], prefix: string) {
  const occupied = new Set(values);
  const cleanPrefix = prefix.trim();
  if (cleanPrefix.length === 0) {
    throw new Error("empty prefix");
  }
  let suffix = 1;
  let result = cleanPrefix;
  while (occupied.has(result)) {
    const nextSuffix = String(suffix).padStart(3, "0");
    result = cleanPrefix + "-" + nextSuffix;
    suffix += 1;
  }
  occupied.add(result);
  return result;
}
