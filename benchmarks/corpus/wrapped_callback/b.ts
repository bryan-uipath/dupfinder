import { useCallback } from 'react'; export const persistEntry = useCallback((value: string) => {
  const trimmed = value.trim();
  const encoded = JSON.stringify({ value: trimmed });
  storage.write(encoded);
  return encoded.length;
}, [storage]);
