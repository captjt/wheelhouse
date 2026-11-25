// Shiki's WebAssembly regex engine, left out: the Changes view highlights
// with its JavaScript engine (Pierre's default, and the app's CSP allows no
// WebAssembly), so this only answers if something asks for it by name.
export function createOnigurumaEngine(): never {
  throw new Error("Shiki's WebAssembly engine isn't in this build; use the JavaScript engine");
}
export default undefined;
