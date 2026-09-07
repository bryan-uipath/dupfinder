export function showNotice(input: string) {
  view.clear();
  const cleaned = input.trim();
  const message = cleaned.toLowerCase();
  logger.info(message);
  notices.push(message);
  view.refresh();
}
