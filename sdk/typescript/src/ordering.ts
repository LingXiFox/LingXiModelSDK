import type { CatalogModel, CatalogProvider } from "./types.ts";

interface NaturalToken {
  isNumber: boolean;
  raw: string;
  trimmed: string;
}

function tokenizeNatural(text: string): NaturalToken[] {
  if (!text) return [];
  const tokens: NaturalToken[] = [];
  let digits = "";
  let words = "";

  const flushDigits = () => {
    if (!digits) return;
    const trimmed = digits.replace(/^0+/, "") || "0";
    tokens.push({ isNumber: true, raw: digits, trimmed });
    digits = "";
  };

  const flushWords = () => {
    if (!words) return;
    tokens.push({ isNumber: false, raw: words.toLowerCase(), trimmed: "" });
    words = "";
  };

  for (const ch of text) {
    if (ch >= "0" && ch <= "9") {
      flushWords();
      digits += ch;
    } else {
      flushDigits();
      words += ch;
    }
  }
  flushDigits();
  flushWords();
  return tokens;
}

export function compareNatural(a: string, b: string): number {
  const left = tokenizeNatural(a);
  const right = tokenizeNatural(b);
  const limit = Math.min(left.length, right.length);
  for (let i = 0; i < limit; i++) {
    const lt = left[i];
    const rt = right[i];
    if (lt.isNumber && !rt.isNumber) return -1;
    if (!lt.isNumber && rt.isNumber) return 1;
    if (lt.isNumber && rt.isNumber) {
      if (lt.trimmed.length !== rt.trimmed.length) {
        return lt.trimmed.length < rt.trimmed.length ? -1 : 1;
      }
      if (lt.trimmed !== rt.trimmed) {
        return lt.trimmed < rt.trimmed ? -1 : 1;
      }
      if (lt.raw !== rt.raw) {
        return lt.raw < rt.raw ? -1 : 1;
      }
    } else if (lt.raw !== rt.raw) {
      return lt.raw < rt.raw ? -1 : 1;
    }
  }
  if (left.length !== right.length) {
    return left.length < right.length ? -1 : 1;
  }
  return 0;
}

export function sortProviders(
  providers: readonly CatalogProvider[],
  featured: readonly string[] = []
): CatalogProvider[] {
  const ranks = new Map<string, number>();
  featured.forEach((id, idx) => ranks.set(id.trim().toLowerCase(), idx));
  return [...providers].sort((a, b) => {
    const ra = ranks.get(a.id.trim().toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    const rb = ranks.get(b.id.trim().toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    if (ra !== rb) return ra - rb;
    const byName = compareNatural(a.name, b.name);
    if (byName !== 0) return byName;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
}

export function modelsInStableOrder(models: readonly CatalogModel[]): CatalogModel[] {
  return [...models].sort((a, b) => {
    const ra = a.releaseDate ?? "";
    const rb = b.releaseDate ?? "";
    if (ra !== rb) {
      if (!ra) return 1;
      if (!rb) return -1;
      return ra > rb ? -1 : 1;
    }
    const byFamily = compareNatural(a.family ?? "", b.family ?? "");
    if (byFamily !== 0) return byFamily;
    const byName = compareNatural(a.name, b.name);
    if (byName !== 0) return byName;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
}
