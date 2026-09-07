export const api = {
  sanitizeLabel: (text: string) => {
    const result = text.trim();
    if (result.length === 0) throw new Error("empty");
    return result.toLowerCase();
  },
};
