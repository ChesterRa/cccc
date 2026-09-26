// Format ISO timestamp to friendly relative/absolute time
export function formatTime(isoStr: string | undefined): string {
  if (!isoStr) return "—";
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSec = Math.floor(diffMs / 1000);
    const diffMin = Math.floor(diffSec / 60);
    const diffHour = Math.floor(diffMin / 60);
    const diffDay = Math.floor(diffHour / 24);
    if (diffSec < 60) return "just now";
    if (diffMin < 60) return `${diffMin}m ago`;
    if (diffHour < 24) return `${diffHour}h ago`;
    if (diffDay < 7) return `${diffDay}d ago`;
    const month = date.toLocaleString("en", { month: "short" });
    const day = date.getDate();
    const year = date.getFullYear();
    const currentYear = now.getFullYear();
    if (year === currentYear) return `${month} ${day}`;
    return `${month} ${day}, ${year}`;
  } catch {
    return isoStr;
  }
}

export function formatFullTime(isoStr: string | undefined): string {
  if (!isoStr) return "";
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    return date.toLocaleString();
  } catch {
    return isoStr;
  }
}
