export function summarizeLengths(items: string[]) {
  let total = 0;
  for (const item of items) {
    total += item.length;
  }
  return total;
}
