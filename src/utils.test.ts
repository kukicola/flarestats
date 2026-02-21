import { describe, it, expect } from "vitest";
import { formatNumber, formatTimestamp, escapeAttr } from "./utils";

describe("formatNumber", () => {
  it("returns plain number below 1000", () => {
    expect(formatNumber(0)).toBe("0");
    expect(formatNumber(1)).toBe("1");
    expect(formatNumber(999)).toBe("999");
  });

  it("formats thousands with K suffix", () => {
    expect(formatNumber(1000)).toBe("1.0K");
    expect(formatNumber(1500)).toBe("1.5K");
    expect(formatNumber(999999)).toBe("1000.0K");
  });

  it("formats millions with M suffix", () => {
    expect(formatNumber(1000000)).toBe("1.0M");
    expect(formatNumber(1500000)).toBe("1.5M");
    expect(formatNumber(10000000)).toBe("10.0M");
  });
});

describe("formatTimestamp", () => {
  it("formats daily timestamps (YYYY-MM-DD) by stripping year", () => {
    expect(formatTimestamp("2024-01-15")).toBe("01-15");
    expect(formatTimestamp("2024-12-01")).toBe("12-01");
  });

  it("formats hourly ISO timestamps to HH:00 in local time", () => {
    // getHours() returns local time, so expected values depend on timezone
    const expected = (iso: string) => {
      const d = new Date(iso);
      return d.getHours().toString().padStart(2, "0") + ":00";
    };
    expect(formatTimestamp("2024-01-15T09:00:00Z")).toBe(expected("2024-01-15T09:00:00Z"));
    expect(formatTimestamp("2024-01-15T23:00:00Z")).toBe(expected("2024-01-15T23:00:00Z"));
    expect(formatTimestamp("2024-01-15T00:00:00Z")).toBe(expected("2024-01-15T00:00:00Z"));
  });

  it("returns short strings as-is", () => {
    expect(formatTimestamp("abc")).toBe("abc");
    expect(formatTimestamp("")).toBe("");
  });
});

describe("escapeAttr", () => {
  it("escapes ampersands", () => {
    expect(escapeAttr("a&b")).toBe("a&amp;b");
  });

  it("escapes double quotes", () => {
    expect(escapeAttr('a"b')).toBe("a&quot;b");
  });

  it("escapes angle brackets", () => {
    expect(escapeAttr("<script>")).toBe("&lt;script&gt;");
  });

  it("escapes all special chars together", () => {
    expect(escapeAttr('<img src="x" onerror="alert(1)&">')).toBe(
      "&lt;img src=&quot;x&quot; onerror=&quot;alert(1)&amp;&quot;&gt;"
    );
  });

  it("returns safe strings unchanged", () => {
    expect(escapeAttr("hello world")).toBe("hello world");
    expect(escapeAttr("")).toBe("");
  });
});
