export function queueAlert(input: string) {
  const id = ids.next();
  const cleaned = input.trim();
  const message = cleaned.toLowerCase();
  logger.info(message);
  notices.push(message);
  return id;
}
