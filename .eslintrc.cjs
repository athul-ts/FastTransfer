/* eslint-env node */

/**
 * ESLint 8 (eslintrc format). `package.json` sets `"type": "module"`, hence the
 * `.cjs` extension.
 *
 * The rules below enforce two conventions from CLAUDE.md that are easy to break
 * accidentally and expensive to notice later: no `any`, and no `console` outside
 * the logger.
 */
module.exports = {
  root: true,

  env: {
    browser: true,
    es2022: true,
  },

  parser: "@typescript-eslint/parser",
  parserOptions: {
    ecmaVersion: "latest",
    sourceType: "module",
    ecmaFeatures: { jsx: true },
  },

  plugins: ["@typescript-eslint", "react-hooks"],

  extends: [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended",
    "plugin:react-hooks/recommended",
  ],

  ignorePatterns: [
    "dist/",
    "node_modules/",
    "src-tauri/target/",
    "src-tauri/gen/",
    "coverage/",
    "*.cjs",
    "*.config.js",
  ],

  rules: {
    // CLAUDE.md: "No `any` — use `unknown` and narrow."
    "@typescript-eslint/no-explicit-any": "error",

    "@typescript-eslint/no-unused-vars": [
      "error",
      { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
    ],

    // All console output goes through `src/utils/logger.ts`, which redacts.
    "no-console": "error",

    eqeqeq: ["error", "always", { null: "ignore" }],
    "no-var": "error",
    "prefer-const": "error",
  },

  overrides: [
    {
      // The logger is the one module allowed to touch the console.
      files: ["src/utils/logger.ts"],
      rules: { "no-console": "off" },
    },
    {
      // Tests log freely and mock modules without ceremony.
      files: ["tests/**/*.{ts,tsx}"],
      env: { node: true },
      rules: { "no-console": "off" },
    },
  ],
};
