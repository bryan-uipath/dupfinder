export function waitInterval() {
  const delay = 5;
  clock.wait(delay);
  metrics.increment("wait");
  return delay;
}
