/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Semantic surface tokens — all wired through CSS variables defined
        // in index.css so the dark theme just flips the var values.
        canvas: 'rgb(var(--bg) / <alpha-value>)',
        elev:   'rgb(var(--bg-elev) / <alpha-value>)',
        sunk:   'rgb(var(--bg-sunk) / <alpha-value>)',
        ink: {
          DEFAULT: 'rgb(var(--ink) / <alpha-value>)',
          2: 'rgb(var(--ink-2) / <alpha-value>)',
          3: 'rgb(var(--ink-3) / <alpha-value>)',
          4: 'rgb(var(--ink-4) / <alpha-value>)',
        },
        line: {
          DEFAULT: 'rgb(var(--line) / <alpha-value>)',
          strong:  'rgb(var(--line-strong) / <alpha-value>)',
        },
        accent: {
          DEFAULT: 'rgb(var(--accent) / <alpha-value>)',
          soft:    'rgb(var(--accent-soft) / <alpha-value>)',
          ink:     'rgb(var(--accent-ink) / <alpha-value>)',
        },
        good:   { DEFAULT: 'rgb(var(--good) / <alpha-value>)',   soft: 'rgb(var(--good-soft) / <alpha-value>)' },
        warn:   { DEFAULT: 'rgb(var(--warn) / <alpha-value>)',   soft: 'rgb(var(--warn-soft) / <alpha-value>)' },
        danger: { DEFAULT: 'rgb(var(--danger) / <alpha-value>)', soft: 'rgb(var(--danger-soft) / <alpha-value>)' },
      },
      fontFamily: {
        sans:   ['Inter', 'ui-sans-serif', 'system-ui', '-apple-system', 'Segoe UI', 'sans-serif'],
        serif:  ['Fraunces', 'Times New Roman', 'serif'],
        mono:   ['"JetBrains Mono"', 'ui-monospace', '"SF Mono"', 'Menlo', 'monospace'],
      },
      boxShadow: {
        soft1: '0 1px 0 rgb(20 20 16 / 0.04), 0 1px 2px rgb(20 20 16 / 0.04)',
        soft2: '0 2px 6px rgb(20 20 16 / 0.06), 0 12px 30px rgb(20 20 16 / 0.08)',
        soft3: '0 30px 60px rgb(20 20 16 / 0.18), 0 8px 24px rgb(20 20 16 / 0.10)',
      },
      borderRadius: {
        md: '6px',
        lg: '10px',
        xl: '14px',
      },
    },
  },
  plugins: [],
};
