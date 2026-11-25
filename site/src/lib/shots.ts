// Screenshots, by name, from the README's own (docs/images): one source, so
// `just screenshots` updates the README and the site together. "inbox"
// finds inbox-light.png and inbox-dark.png; a name with one file
// ("social-preview") is the same in both modes. A name with neither fails
// the build.
import type { ImageMetadata } from "astro";

// Not eager: each page loads only the captures it shows, so the build ships
// no image nothing uses.
const images = import.meta.glob<ImageMetadata>("../../../docs/images/*.png", { import: "default" });
const find = (file: string): Promise<ImageMetadata> | undefined => images[`../../../docs/images/${file}.png`]?.();

export type Shot = {
  light: ImageMetadata;
  dark: ImageMetadata;
  /** The capture's pixels per CSS pixel: its natural size is width / density. */
  density: number;
  phone: boolean;
};

export async function shot(name: string): Promise<Shot> {
  const light = await (find(`${name}-light`) ?? find(name));
  const dark = await (find(`${name}-dark`) ?? find(name));
  if (!light || !dark) throw new Error(`no screenshot "${name}" in docs/images`);
  const phone = light.height > light.width * 1.5;
  // The web app's captures are 2x (2880 wide), its phone ones 3x; the Mac
  // app's own window captures (1440 and under) are 1x.
  const density = phone ? 3 : light.width >= 2400 ? 2 : 1;
  return { light, dark, density, phone };
}

/** Widths to generate: never wider than the file (Astro doesn't upscale). */
export function widths(src: ImageMetadata, want: number[]): number[] {
  const fit = want.filter((w) => w < src.width);
  return [...new Set([...fit, src.width])];
}
