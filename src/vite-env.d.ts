/// <reference types="vite/client" />

/** The change log, generated from git at build time by `vite.config.ts`. */
declare module "virtual:changelog" {
  export const changelog: import("./about").ChangelogEntry[];
}
