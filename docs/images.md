# Images for agents

Show the agents on a docket what you mean: a mockup of what it should look
like, a screenshot of something you like, or a screenshot you marked up
with boxes, arrows and numbered pins. Agents see the image itself, not a
description of it.

## Sending one

In a docket's composer, in the web app or Wheelhouse.app:

- **Paste** an image (⌘V), **drop** one on the composer, or press the
  paperclip to pick some. On a phone the paperclip opens Photos and the
  camera. At most 10 go at once, each up to 15 MB. A phone's HEIC, an
  AVIF, or anything too big is redrawn as a JPEG or PNG first.
- Give each one a **caption**: what to look at.
- **Mark it up** (click the image): box `B`, arrow `A`, pen `P`, text `T`,
  numbered pin `N`, colors `1`–`6`, ⌘Z to undo. Each pin has a note at the
  side. The agents get the marked-up image at full size, plus what each pin
  says as text, and the docket keeps the original beside it.
- Then send it the way you'd send text: `@builder match this` gives it to
  that agent with the prompt, `/ask Which of these? | A | B` asks with it,
  `/decide Header: like this` records a decision with it, `/task` starts an
  agent with it, and plain text (or no text at all) keeps it on the docket
  as a note.

Once it's sent, every image is an artifact on the docket's record, under
**References**, pinned by its hash like any other kept file. The entry it
went with (the note, the question, the decision) pins it too. A command the
hub refuses (an `/ask` with no agent on the docket, say) keeps nothing.

## What agents get

The hub writes each image to `~/.local/state/wheelhouse/images/D-N/` (only
you can read it) and adds its path to the text the agent gets:

```
match this

An image comes with this (open it to see it: Read in Claude Code, view_image in Codex):
1. /Users/you/.local/state/wheelhouse/images/D-12/3f2a91c0b7de-settings.png (1440×900): The header, like this
   Numbered pins on it: 1. Make this the primary button; 2. Too much padding
```

Claude Code opens it with Read, Codex with view_image, and Pi with read,
whether they run in Rex or in Herdr. On a shared docket, each teammate's hub
writes the image out on their own Mac, so their agents' paths work too.

`wh brief` lists a docket's images under **Images**, so an agent that joins
later sees them as well. `wh images` writes them all out and prints their
paths.

## In the intent

Paste or drop a mockup into the intent while you edit it, or use the image
button in its toolbar. It goes in as Markdown
(`![The settings page](/api/blobs/sha256:…)`), shows in both apps, and is
kept with the docket when you save. The intent's entry pins it. In
`wh brief`, the link becomes the image's path, so the spec an agent reads
points at a file it can open.

## Comparing what was built

An agent that built what a reference shows can attach a screenshot of its
work against it:

```sh
wh attach shot.png --against 12
```

Open either image on the docket to see the two **side by side**, as a
**swipe**, or as an **onion skin** (one over the other, with a slider). You
can also compare any image with any other on the docket.

## From the phone's share sheet

**Android, and Chrome on desktop.** Install the web app (Add to Home
Screen). Wheelhouse then shows up in the share sheet: share a screenshot to
it, pick a docket (or start a new one), and the image waits in that docket's
composer for you to caption, mark up and send.

**iPhone and iPad.** Safari doesn't let a web app show up in the share
sheet. A Shortcut does the same job:

1. In Shortcuts, make a new shortcut. In its details, turn on **Show in
   Share Sheet** and accept **Images**.
2. **Repeat with Each** item in **Shortcut Input**:
   - **Convert Image** (the Repeat Item) to **JPEG**: the hub doesn't take HEIC.
   - **Get Contents of URL**: `https://<your Mac>.<tailnet>.ts.net/api/images?name=shared.jpg`,
     Method **POST**, Header `X-Wheelhouse` = `shortcut`, Request Body
     **File**: the Converted Image.
   - **Get Dictionary Value** `digest` from the Contents of URL.
3. **Combine Text** from the Repeat Results, with a custom separator `,`.
4. **Open URLs**: `https://<your Mac>.<tailnet>.ts.net/#/share/` followed by
   the Combined Text.

Your hub's address is the one `wh web --publish` printed. It answers only
you, on your tailnet, so the Shortcut needs no key. The images are only
staged until you send them from a docket: nothing goes on a record before
then.

## Limits

- PNG, JPEG, GIF and WebP, checked by their bytes, not their names. SVG is
  never taken as an image, because it can run scripts.
- At most 15 MB per image, 20,000 pixels on a side, and 100 megapixels in
  all. At most 10 images per message, and 40 pins per image.
- Captions and pin notes are one line each, at most 500 characters, and at
  most 4,000 characters in all for one message's images.
- An image staged and never sent is let go of after three days. A file
  written out for agents that hasn't changed in two weeks is removed, and
  written again when an agent next needs it.

The API is `POST /api/images` (the bytes as the body; it answers
`{digest, name, media_type, width, height, size}`), then
`POST /api/dockets/{id}/compose` with `images: [{digest, name, caption,
original?, pins?: [{n, note}]}]`. Both are in `src/images.rs` and
`src/server.rs`.
