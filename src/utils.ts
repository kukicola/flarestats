export function formatNumber(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + "M";
  if (n >= 1000) return (n / 1000).toFixed(1) + "K";
  return n.toString();
}

export function formatTimestamp(ts: string): string {
  if (ts.length === 10) return ts.substring(5);
  if (ts.length > 10) {
    const d = new Date(ts);
    return d.getHours().toString().padStart(2, "0") + ":00";
  }
  return ts;
}

export function escapeAttr(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
