export class Reader {
  encodeValue = (input: string) => {
    const normalized = input.trim();
    const result = JSON.stringify(normalized);
    return result.length;
  };
}
