export function encodePairs(keys: string[], values: string[]) {
  const result = new Map();
  for (let index = 0; index < keys.length; index++) {
    result.set(keys[index], values[index]);
  }
  return result;
}
