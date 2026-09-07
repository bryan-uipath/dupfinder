export function makeDictionary(names: string[], entries: string[]) {
  const mapping = new Map();
  for (let position = 0; position < names.length; position++) {
    mapping.set(names[position], entries[position]);
  }
  return mapping;
}
