## 2026-09-26 - Focus Ring Contrast Issue
**Learning:** The memory says 'When styling focus rings with Tailwind in panel-ui, avoid low-opacity colors (like /50) on dark backgrounds as they fail accessibility contrast checks. Use explicit dark: variants with solid colors instead (e.g., focus-visible:ring-blue-500 dark:focus-visible:ring-blue-400 focus-visible:outline-none).'
**Action:** Update ThemeToggle.tsx to follow this rule.
