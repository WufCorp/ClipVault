// Распознавание URL, чистка UTM-меток и извлечение домена (Pro, 3.3/4.4).

const TRACKING_PARAMS = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_term",
  "utm_content",
  "utm_id",
  "gclid",
  "fbclid",
  "yclid",
  "mc_eid",
  "mc_cid",
  "igshid",
  "ref",
  "ref_src",
  "_ga",
  "spm",
];

/// Возвращает URL, если строка целиком является ссылкой http(s), иначе null.
export function parseUrl(input: string): URL | null {
  const s = input.trim();
  if (s.length === 0 || s.length > 2048 || /\s/.test(s)) return null;
  if (!/^https?:\/\//i.test(s)) return null;
  try {
    return new URL(s);
  } catch {
    return null;
  }
}

/// Возвращает адрес, если строка целиком — email (можно с "mailto:"), иначе null.
export function parseEmail(input: string): string | null {
  const s = input.trim().replace(/^mailto:/i, "");
  if (s.length === 0 || s.length > 254) return null;
  return /^[^\s@<>()",;:]+@[^\s@<>()",;:]+\.[^\s@<>()",;:.]{2,}$/.test(s) ? s : null;
}

/// Домен ссылки без "www.".
export function domainOf(u: URL): string {
  return u.hostname.replace(/^www\./, "");
}

/// Убирает трекинговые/UTM-параметры. Возвращает очищенную строку URL.
export function cleanUrl(u: URL): string {
  const clone = new URL(u.toString());
  for (const p of TRACKING_PARAMS) clone.searchParams.delete(p);
  // Также удаляем любые параметры, начинающиеся на "utm_".
  for (const key of [...clone.searchParams.keys()]) {
    if (key.toLowerCase().startsWith("utm_")) clone.searchParams.delete(key);
  }
  let out = clone.toString();
  // URL.toString() оставляет висячий "?" если параметров не осталось.
  out = out.replace(/\?$/, "");
  return out;
}
