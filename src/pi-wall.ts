// Wheelhouse's wall for Pi, in Review's and Build's panes (docs/build.md,
// "The wall"). Pi asks before nothing, so the commands a reviewer, checker
// or builder may not run are refused here, before they run: each one in
// `WH_PI_DENY` (a JSON list, such as ["git push", "gh"]), matched by whole
// words at the start of any command in what the agent runs. The rest of the
// wall (no tokens, pushes that fail, no MCP, no other extensions) is the
// pane's own: this is the part Claude Code's `--disallowedTools` does.
//
// Wheelhouse writes this file and passes it with `-e`; edits are lost.
// It stops mistakes, not a hostile agent (docs/security.md).
//
// wheelhouse-pi-wall v1

/** Words before a command that leave it the same command. */
const WRAPPERS = new Set(["env", "command", "exec", "nohup", "time", "nice", "builtin", "sudo", "doas", "xargs"]);
/** Shells whose `-c` runs the words after it as a command. */
const SHELLS = new Set(["sh", "bash", "zsh", "dash", "ksh", "fish"]);

/** The denied commands, as words; null when the list can't be read (then nothing runs). */
function readDeny(raw: string | undefined): string[][] | null {
  if (raw === undefined || raw.trim() === "") return [];
  try {
    const v = JSON.parse(raw);
    if (!Array.isArray(v) || !v.every((x) => typeof x === "string")) return null;
    return v.map((x: string) => x.trim().split(/\s+/).filter(Boolean)).filter((w: string[]) => w.length > 0);
  } catch {
    return null;
  }
}

const deny = readDeny(process.env.WH_PI_DENY);

/** Each simple command in a command line, as its words (quotes taken off). */
export function commands(line: string): string[][] {
  return line
    .split(/\$\(|<\(|>\(|[;&|\n`(){}]/)
    .map((seg) => seg.split(/\s+/).map((w) => w.replace(/["'\\]/g, "")).filter(Boolean))
    .filter((w) => w.length > 0);
}

/** A command's words from the program on: no `VAR=x`, wrappers, or path. */
function program(words: string[]): string[] {
  let i = 0;
  while (i < words.length) {
    const w = words[i];
    const base = w.slice(w.lastIndexOf("/") + 1);
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(w)) i++;
    else if (WRAPPERS.has(base)) {
      i++;
      // A wrapper's own options (`env -i`, `nice -n 5`, `sudo -u x`).
      while (i < words.length && words[i].startsWith("-")) i += /^-[nuCgp]$/.test(words[i]) ? 2 : 1;
    } else if (SHELLS.has(base) && /^-[a-z]*c$/.test(words[i + 1] ?? "")) i += 2;
    else if (base === "eval") i++;
    else break;
  }
  const rest = words.slice(i);
  if (rest.length > 0) rest[0] = rest[0].slice(rest[0].lastIndexOf("/") + 1);
  return rest;
}

/** The denied command `line` runs, if it runs one. */
export function denied(line: string, list: string[][]): string | undefined {
  for (const c of commands(line)) {
    const words = program(c);
    for (const d of list) {
      if (d.length <= words.length && d.every((w, i) => words[i] === w)) return d.join(" ");
    }
  }
  return undefined;
}

export default function wall(pi: any): void {
  pi.on("tool_call", (event: any) => {
    // A script that calls tools itself: its calls wouldn't come through here.
    if (event?.toolName === "codemode") {
      return { block: true, reason: "Wheelhouse's wall: codemode is off in Wheelhouse's panes. Use the tools directly." };
    }
    const line = event?.input?.command;
    if (typeof line !== "string") return;
    if (deny === null) {
      return { block: true, reason: "Wheelhouse's wall couldn't read which commands are denied (WH_PI_DENY), so none run. Tell the user." };
    }
    const hit = denied(line, deny);
    if (hit) {
      return { block: true, reason: `Wheelhouse's wall: \`${hit}\` isn't for this agent. Wheelhouse does that step itself; leave it and go on.` };
    }
  });
}
