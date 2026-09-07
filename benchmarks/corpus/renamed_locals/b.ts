export function measureText(rows: string[]) {
  let count = 0;
  for (const row of rows) {
    count += row.length;
  }
  return count;
}
