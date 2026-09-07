export function previewMessage(input: string) {
  telemetry.record();
  const clean = input.trim();
  const payload = JSON.stringify({ text: clean });
  storage.write(payload);
  cache.invalidate();
  return payload;
}
