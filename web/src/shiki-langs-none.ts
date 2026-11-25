// The highlighter's worker never resolves a language itself (the page does,
// and hands the grammar over), so its copy of shiki/langs is empty
// (vite.config.ts).
import type { DynamicImportLanguageRegistration } from "shiki";

export const bundledLanguages: Record<string, DynamicImportLanguageRegistration> = {};
