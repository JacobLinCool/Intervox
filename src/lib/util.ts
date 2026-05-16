/** Turn a style object into a CSS string. Number values that are lengths get
 *  "px"; the keys in UNITLESS stay unitless. `false`, `""`, `null`, and
 *  `undefined` values are dropped — this lets React-port call sites write
 *  `{ display: active && "flex" }` and have the falsy branch disappear. */
const UNITLESS = new Set([
  "opacity", "zIndex", "fontWeight", "lineHeight", "flex", "flexGrow",
  "flexShrink", "order", "zoom",
]);
const kebab = (k: string) => k.replace(/[A-Z]/g, (m) => "-" + m.toLowerCase());
export function css(
  obj: Record<string, string | number | boolean | undefined | null>,
): string {
  return Object.entries(obj)
    .filter(([, v]) => v !== undefined && v !== null && v !== false && v !== "")
    .map(([k, v]) =>
      `${kebab(k)}:${typeof v === "number" && !UNITLESS.has(k) ? `${v}px` : v}`)
    .join(";");
}
