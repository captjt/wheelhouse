// Screenshots, by name, from the README's own (docs/images): one source, so
// `just screenshots` updates the README and the site together. "inbox"
// finds inbox-light.png and inbox-dark.png; a name with one file
// ("social-preview") is the same in both modes. A name with neither fails
// the build.
import type { ImageMetadata } from "astro";
import sharp from "sharp";

// docs/images on disk, from astro.config.mjs.
declare const __WH_IMAGES__: string;

// Not eager: each page loads only the captures it shows, so the build ships
// no image nothing uses.
const images = import.meta.glob<ImageMetadata>("../../../docs/images/*.png", { import: "default" });
const find = (file: string): Promise<ImageMetadata> | undefined => images[`../../../docs/images/${file}.png`]?.();

/** A phone capture's top and bottom edge, for the status bar and home
 * indicator drawn around it: the capture has neither, so the frame does. */
export type Edges = {
  top: string;
  bottom: string;
  /** Whether each edge is dark, for white ink on it. */
  topDark: boolean;
  bottomDark: boolean;
};

export type Shot = {
  light: ImageMetadata;
  dark: ImageMetadata;
  /** The capture's pixels per CSS pixel: its natural size is width / density. */
  density: number;
  phone: boolean;
  /** A phone capture's edges, per mode. */
  edges?: { light: Edges; dark: Edges };
};

export async function shot(name: string): Promise<Shot> {
  const lightFile = images[`../../../docs/images/${name}-light.png`] ? `${name}-light` : name;
  const darkFile = images[`../../../docs/images/${name}-dark.png`] ? `${name}-dark` : name;
  const light = await find(lightFile);
  const dark = await find(darkFile);
  if (!light || !dark) throw new Error(`no screenshot "${name}" in docs/images`);
  const phone = light.height > light.width * 1.5;
  // The web app's captures are 2x (2880 wide), its phone ones 3x; the Mac
  // app's own window captures (1440 and under) are 1x.
  const density = phone ? 3 : light.width >= 2400 ? 2 : 1;
  const edges = phone ? { light: await edgesOf(lightFile), dark: await edgesOf(darkFile) } : undefined;
  return { light, dark, density, phone, edges };
}

const edgeCache = new Map<string, Promise<Edges>>();

function edgesOf(file: string): Promise<Edges> {
  let e = edgeCache.get(file);
  if (!e) {
    e = readEdges(`${__WH_IMAGES__}${file}.png`);
    edgeCache.set(file, e);
  }
  return e;
}

async function readEdges(path: string): Promise<Edges> {
  const { width, height } = await sharp(path).metadata();
  if (!width || !height) throw new Error(`can't read the size of ${path}`);
  // The mean of the outermost row: a capture's edge is one flat color.
  const row = async (top: number) => {
    const { channels } = await sharp(path).extract({ left: 0, top, width, height: 1 }).removeAlpha().stats();
    const [r, g, b] = channels.slice(0, 3).map((c) => Math.round(c.mean));
    const hex = `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
    // Luma, roughly: under the middle reads as dark.
    const dark = 0.2126 * r + 0.7152 * g + 0.0722 * b < 128;
    return { hex, dark };
  };
  const [t, b] = await Promise.all([row(0), row(height - 1)]);
  return { top: t.hex, bottom: b.hex, topDark: t.dark, bottomDark: b.dark };
}

/** Widths to generate: never wider than the file (Astro doesn't upscale). */
export function widths(src: ImageMetadata, want: number[]): number[] {
  const fit = want.filter((w) => w < src.width);
  return [...new Set([...fit, src.width])];
}
