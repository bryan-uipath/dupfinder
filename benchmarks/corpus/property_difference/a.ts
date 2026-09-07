export function collectEntries(store: Store) {
  const values = store.active;
  const ordered = values.sort();
  return ordered.join(",");
}
