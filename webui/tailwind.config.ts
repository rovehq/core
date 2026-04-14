import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./src/**/*.{js,ts,jsx,tsx,mdx}"],
  theme: {
    extend: {
      colors: {
        background: "#0d1114",
        surface: "#151b20",
        surface2: "#23303a",
        primary: "#de6947",
        success: "#2fb28b",
        error: "#e16666",
        warning: "#d5a24d",
      },
    },
  },
  plugins: [],
};
export default config;
