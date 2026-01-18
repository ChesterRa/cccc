import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      // Z-index scale for consistent layering
      // Usage: z-base, z-header, z-dropdown, z-sidebar, z-modal, z-overlay, z-tooltip, z-max
      zIndex: {
        'base': '10',      // Sticky elements, basic UI
        'header': '20',    // App header
        'dropdown': '30',  // Dropdown menus, popover
        'sidebar': '40',   // Sidebar, secondary notifications
        'modal': '50',     // Modal dialogs
        'overlay': '60',   // Full-screen overlays (drag-drop)
        'tooltip': '200',  // Floating tooltips
        'max': '9999',     // Maximum priority (rare)
      },
      colors: {
        // Semantic colors using CSS variables
        bg: {
          primary: "var(--color-bg-primary)",
          secondary: "var(--color-bg-secondary)",
          tertiary: "var(--color-bg-tertiary)",
          elevated: "var(--color-bg-elevated)",
        },
        border: {
          primary: "var(--color-border-primary)",
          secondary: "var(--color-border-secondary)",
          focus: "var(--color-border-focus)",
        },
        text: {
          primary: "var(--color-text-primary)",
          secondary: "var(--color-text-secondary)",
          tertiary: "var(--color-text-tertiary)",
          muted: "var(--color-text-muted)",
          inverse: "var(--color-text-inverse)",
        },
        accent: {
          primary: "var(--color-accent-primary)",
          secondary: "var(--color-accent-secondary)",
          warning: "var(--color-accent-warning)",
          danger: "var(--color-accent-danger)",
        },
      },
      animation: {
        "fade-in": "fade-in 0.2s ease-out",
        "slide-up": "slide-up 0.25s ease-out",
        "scale-in": "scale-in 0.2s ease-out",
        "pulse-slow": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
      },
      transitionTimingFunction: {
        "out-expo": "cubic-bezier(0.16, 1, 0.3, 1)",
      },
      fontFamily: {
        sans: ["Inter", "ui-sans-serif", "system-ui", "-apple-system", "BlinkMacSystemFont", "Segoe UI", "Roboto", "Helvetica Neue", "Arial", "sans-serif"],
      },
    },
  },
  plugins: [],
} satisfies Config;

