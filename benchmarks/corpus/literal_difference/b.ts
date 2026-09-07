export function waitInterval() {
  const delay = 50;
  clock.wait(delay);
  metrics.increment("wait");
  return delay;
}
