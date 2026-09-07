export function persistReport(input: string) {
  audit.start();
  const clean = input.trim();
  const payload = JSON.stringify({ text: clean });
  storage.write(payload);
  cache.invalidate();
  return payload.length;
}
