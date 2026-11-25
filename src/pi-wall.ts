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
const WRAPPERS = new Set(["env", "command", "exec", "nohup", "time", "nice", "builtin", "sudo", "doas", "xargs", "timeout", "stdbuf", "ionice", "caffeinate", "unbuffer"]);
/** A wrapper's options that take the next word (`xargs -P 4`, `sudo -u me`, `timeout -k 5`). */
const TAKES_VALUE = /^-[nuCgpPILdEsak]$/;
/** Shells whose `-c` runs the word after it as a command line. */
const SHELLS = new Set(["sh", "bash", "zsh", "dash", "ksh", "fish"]);
/** git's own options that take the next word, before its command. */
const GIT_VALUE = /^(-C|-c|--git-dir|--work-tree|--namespace|--exec-path|--super-prefix|--config-env)$/;
/** How deep `bash -c "…"` and `$( … )` are followed. */
const MAX_DEPTH = 8;

const base = (w: string) => w.slice(w.lastIndexOf("/") + 1);
const assignment = (w: string) => /^[A-Za-z_][A-Za-z0-9_]*=/.test(w);

/** The readable denied list, as words; null when it can't be read (then nothing runs). */
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

/**
 * Each simple command a shell would run for `line`, as its words, read the
 * way a shell reads them: quoted text and heredoc bodies are words, not
 * commands (`git commit -m "(gh can't)"` runs git), while `$( … )` and
 * backticks run, in double quotes too, as do `bash -c "…"`, `eval …` and a
 * heredoc a shell reads.
 */
export function commands(line: string, depth = 0): string[][] {
  const out: string[][] = [];
  if (depth > MAX_DEPTH) return out;
  const n = line.length;
  let words: string[] = [];
  let word: string | null = null;
  const heredocs: { tag: string; strip: boolean; shell: boolean }[] = [];
  const endWord = () => {
    if (word !== null) words.push(word);
    word = null;
  };
  const endCommand = () => {
    endWord();
    if (words.length) out.push(...run(words, depth));
    words = [];
  };
  const sub = (inner: string) => out.push(...commands(inner, depth + 1));
  // Where the `)` closing a `(` just before `j` is.
  const close = (j: number): number => {
    let d = 1;
    let q: string | null = null;
    for (; j < n; j++) {
      const c = line[j];
      if (q) {
        if (c === "\\" && q === '"') j++;
        else if (c === q) q = null;
      } else if (c === "'" || c === '"') q = c;
      else if (c === "\\") j++;
      else if (c === "(") d++;
      else if (c === ")" && --d === 0) return j;
    }
    return n;
  };
  const tick = (j: number) => {
    const e = line.indexOf("`", j);
    return e < 0 ? n : e;
  };
  let i = 0;
  while (i < n) {
    const c = line[i];
    if (c === "\\") {
      if (line[i + 1] !== "\n") word = (word ?? "") + (line[i + 1] ?? "");
      i += 2;
    } else if (c === "'") {
      const e = line.indexOf("'", i + 1);
      const end = e < 0 ? n : e;
      word = (word ?? "") + line.slice(i + 1, end);
      i = end + 1;
    } else if (c === '"') {
      let w = word ?? "";
      let j = i + 1;
      while (j < n && line[j] !== '"') {
        if (line[j] === "\\") {
          w += line[j + 1] ?? "";
          j += 2;
        } else if (line[j] === "$" && line[j + 1] === "(") {
          const e = close(j + 2);
          sub(line.slice(j + 2, e));
          j = e + 1;
        } else if (line[j] === "`") {
          const e = tick(j + 1);
          sub(line.slice(j + 1, e));
          j = e + 1;
        } else w += line[j++];
      }
      word = w;
      i = j + 1;
    } else if ((c === "$" || c === "<" || c === ">") && line[i + 1] === "(") {
      // `$( … )`, and process substitution `<( … )`: commands that run.
      const e = close(i + 2);
      sub(line.slice(i + 2, e));
      word = (word ?? "") + "$";
      i = e + 1;
    } else if (c === "$" && line[i + 1] === "{") {
      const e = line.indexOf("}", i);
      const end = e < 0 ? n : e;
      word = (word ?? "") + line.slice(i, end + 1);
      i = end + 1;
    } else if (c === "`") {
      const e = tick(i + 1);
      sub(line.slice(i + 1, e));
      word = (word ?? "") + "$";
      i = e + 1;
    } else if (c === "<" && line[i + 1] === "<" && line[i + 2] !== "<") {
      // A heredoc: its body, from the next line, is text, unless a shell reads it.
      endWord();
      let j = i + 2;
      const strip = line[j] === "-";
      if (strip) j++;
      while (line[j] === " " || line[j] === "\t") j++;
      let tag = "";
      while (j < n && !/[\s;&|<>()]/.test(line[j])) {
        if (!"'\"\\".includes(line[j])) tag += line[j];
        j++;
      }
      const prog = words.find((w) => !assignment(w));
      if (tag) heredocs.push({ tag, strip, shell: prog !== undefined && SHELLS.has(base(prog)) });
      i = j;
    } else if (c === "#" && word === null) {
      const e = line.indexOf("\n", i);
      i = e < 0 ? n : e;
    } else if (c === "\n") {
      endCommand();
      i++;
      for (const h of heredocs) {
        const body: string[] = [];
        while (i < n) {
          const e = line.indexOf("\n", i);
          const end = e < 0 ? n : e;
          const l = line.slice(i, end);
          i = end + 1;
          if ((h.strip ? l.replace(/^\t+/, "") : l) === h.tag) break;
          body.push(l);
        }
        if (h.shell) sub(body.join("\n"));
      }
      heredocs.length = 0;
    } else if (";&|(){}".includes(c)) {
      endCommand();
      i++;
    } else if (c === " " || c === "\t") {
      endWord();
      i++;
    } else {
      word = (word ?? "") + c;
      i++;
    }
  }
  endCommand();
  return out;
}

/** What one simple command runs: its program's words, or the commands a shell or `eval` runs. */
function run(words: string[], depth: number): string[][] {
  let i = 0;
  while (i < words.length) {
    const w = words[i];
    const b = base(w);
    if (assignment(w)) i++;
    else if (b === "command" && /^-[vV]$/.test(words[i + 1] ?? "")) return []; // a lookup, not a run
    else if (WRAPPERS.has(b)) {
      i++;
      while (i < words.length && words[i].startsWith("-") && words[i] !== "--") i += TAKES_VALUE.test(words[i]) ? 2 : 1;
      if (words[i] === "--") i++;
      // `timeout 60 gh …`: its duration.
      if (b === "timeout" && i < words.length) i++;
    } else if (SHELLS.has(b)) {
      for (let j = i + 1; j < words.length && /^[-+]/.test(words[j]); j++) {
        if (/^[-+][oO]$/.test(words[j])) j++;
        else if (/^-[A-Za-z]*c[A-Za-z]*$/.test(words[j])) return commands(words[j + 1] ?? "", depth + 1);
      }
      return []; // a shell running a script: its commands aren't in this line
    } else if (b === "eval") return commands(words.slice(i + 1).join(" "), depth + 1);
    else break;
  }
  const rest = words.slice(i);
  if (rest.length === 0) return [];
  rest[0] = base(rest[0]);
  if (rest[0] === "git") {
    let j = 1;
    while (j < rest.length && rest[j].startsWith("-")) j += GIT_VALUE.test(rest[j]) ? 2 : 1;
    return [["git", ...rest.slice(j)]];
  }
  return [rest];
}

/** The denied command `line` runs, if it runs one. */
export function denied(line: string, list: string[][]): string | undefined {
  for (const words of commands(line)) {
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
