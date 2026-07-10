import { describe, expect, it } from "vitest";

import {
  DEFAULT_THEME_PREFERENCE,
  resolveThemePreference,
} from "./settings-helpers";

describe("theme preference", () => {
  it("defaults to light independently of the operating-system theme", () => {
    expect(DEFAULT_THEME_PREFERENCE).toBe("Light");
    expect(resolveThemePreference("", false)).toBe("light");
    expect(resolveThemePreference("Unexpected", false)).toBe("light");
  });

  it("respects explicit light, dark, and system preferences", () => {
    expect(resolveThemePreference("Light", false)).toBe("light");
    expect(resolveThemePreference("Dark", true)).toBe("dark");
    expect(resolveThemePreference("System", true)).toBe("light");
    expect(resolveThemePreference("System", false)).toBe("dark");
  });
});
