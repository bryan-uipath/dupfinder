export function uniqueEdge(values: string[], prefix: string) {
  let suffix = 1;
  let result = prefix;
  while (values.includes(result)) {
    result = prefix + suffix;
    suffix += 1;
  }
  return result;
}
