const MIN_DBFS = -80;
const FULL_SCALE_DBFS = -18;

export function rmsToDbfs(rms: number): number | null {
  if (!Number.isFinite(rms) || rms <= 0) return null;
  return 20 * Math.log10(Math.max(rms, 1e-9));
}

export function formatDbfs(rms: number): string {
  const dbfs = rmsToDbfs(rms);
  if (dbfs == null) return "-inf dBFS";
  return `${Math.round(dbfs)} dBFS`;
}

export function rmsToVuLevel(rms: number): number {
  const dbfs = rmsToDbfs(rms);
  if (dbfs == null) return 0;

  const normalized = (dbfs - MIN_DBFS) / (FULL_SCALE_DBFS - MIN_DBFS);
  return Math.max(0, Math.min(1, normalized));
}
