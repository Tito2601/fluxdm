import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: ["class"],
  content: [
    "./pages/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./app/**/*.{ts,tsx}",
    "./src/**/*.{ts,tsx}",
    "./index.html",
  ],
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        // Layered surfaces, darkest (chrome) to lightest (hover / selected).
        surface: {
          0: "#080b11",
          1: "#0d1219",
          2: "#141b25",
          3: "#1c2532",
          4: "#26313f",
        },
        line: {
          DEFAULT: "#1f2836",
          strong: "#2c3849",
        },
        // Semantic accents. `up` marks upload/seeding so it never reads as download.
        accent: {
          DEFAULT: "#4c8dff",
          dim: "#2c5fb3",
          soft: "#8fb8ff",
        },
        up: "#f59e0b",
        ok: "#22c55e",
        warn: "#eab308",
        danger: "#ef4444",

        flux: {
          blue: "#2563eb",
          purple: "#7c3aed",
          dark: "#0f172a",
          darker: "#020617",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
        "pulse-download": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.5" },
        },
        // Moving hatch overlaid on an in-progress bar, so motion signals activity
        // even when the percentage barely changes.
        "progress-stripes": {
          from: { backgroundPosition: "0 0" },
          to: { backgroundPosition: "28px 0" },
        },
        // Sweep used when total size is unknown (magnet metadata, chunked HTTP).
        indeterminate: {
          from: { transform: "translateX(-100%)" },
          to: { transform: "translateX(400%)" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
        "pulse-download": "pulse-download 1.5s ease-in-out infinite",
        "progress-stripes": "progress-stripes 0.7s linear infinite",
        indeterminate: "indeterminate 1.4s ease-in-out infinite",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};

export default config;
