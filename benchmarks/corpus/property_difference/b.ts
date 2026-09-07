export function collectEntries(store: Store) {
  const values = store.archived;
  const ordered = values.sort();
  return ordered.join(",");
}
