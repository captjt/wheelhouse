//! Tab completion through the real shells: `wh completions zsh|bash`, then
//! the hook's function run as a Tab would run it, against a stand-in hub on
//! a socket of its own. Checks what reaches the shell, not just the engine.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;

const WH: &str = env!("CARGO_BIN_EXE_wh");

/// A hub that answers the reads a Tab makes, and nothing else.
fn hub(dir: &Path) -> PathBuf {
    let sock = dir.join("wh.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let line = String::from_utf8_lossy(&req).lines().next().unwrap_or_default().to_string();
            let path = line.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = match path.split('?').next().unwrap_or_default() {
                "/api/state" => (
                    200,
                    r#"{"dockets": [
                        {"id": "D-14", "status": "in_progress", "title": "Retry the uploads"},
                        {"id": "D-13", "status": "done", "title": "Fix the login"},
                        {"id": "D-12", "status": "todo", "title": "Rate limits"}],
                       "agents": [{"block": "block:aaaa-111111", "rex_session": "api", "pane": "planner", "app": "claude-code", "state": "working", "docket": "D-14"}]}"#,
                ),
                "/api/messages" => (200, r#"[{"id": 41, "from": "api/planner", "subject": "Migration order", "body": "", "options": ["Users first", "Together"]}]"#),
                "/api/review" => {
                    (200, r#"{"config": {"repos": ["acme/api"]}, "prs": [{"full_key": "acme/api#12", "repo": "acme/api", "pr": 12, "state": "posted", "title": "Add retries"}]}"#)
                }
                _ => (404, r#"{"error": "not here"}"#),
            };
            let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    sock
}

/// Run `wh completions SHELL`'s hook in SHELL, completing the last word of
/// `line`. What the shell got, one candidate a line, and what it printed
/// on stderr (which, in bash, lands in the prompt).
fn tab(shell: &str, sock: &Path, line: &[&str]) -> (Vec<String>, String) {
    let name = Path::new(shell).file_name().unwrap().to_string_lossy();
    let harness = if name == "zsh" {
        // _describe is what puts candidates in front of you: print them instead.
        r#"eval "$("$WH" completions zsh)"; _describe() { print -rl -- "${(@P)3}"; }
           words=("$@"); CURRENT=$#; _clap_dynamic_completer_wh; true"#
    } else {
        // The function `complete` names for wh: on bash 3, the hook's wrapper.
        r#"eval "$("$WH" completions bash)"; COMP_WORDS=("$@"); COMP_CWORD=$(( $# - 1 )); COMP_TYPE=9
           f=$(complete -p wh | sed 's/.*-F \([^ ]*\).*/\1/'); $f wh "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"; printf '%s\n' "${COMPREPLY[@]}""#
    };
    let mut cmd = Command::new(shell);
    if name == "zsh" {
        cmd.arg("-f");
    } else {
        cmd.args(["--noprofile", "--norc"]);
    }
    let out = cmd.arg("-c").arg(harness).arg(&*name).args(line).env("WH", WH).env("WH_SOCKET", sock).env_remove("REX_BLOCK").env_remove("WH_COMPLETE").output().unwrap();
    assert!(out.status.success(), "{shell} {line:?}: {}", String::from_utf8_lossy(&out.stderr));
    let got = String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    (got, String::from_utf8_lossy(&out.stderr).into_owned())
}

fn shells() -> Vec<String> {
    ["/bin/zsh", "/usr/bin/zsh", "/opt/homebrew/bin/zsh", "/bin/bash", "/opt/homebrew/bin/bash", "/usr/local/bin/bash"]
        .into_iter()
        .filter(|p| Path::new(p).exists())
        .map(String::from)
        .collect()
}

#[test]
fn zsh_and_bash_complete_from_the_hub() {
    let dir = std::env::temp_dir().join(format!("wh-tab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = hub(&dir);
    let down = dir.join("nobody.sock");
    let shells = shells();
    assert!(!shells.is_empty(), "no zsh or bash to try");
    for sh in &shells {
        let zsh = sh.ends_with("zsh");
        // zsh shows each candidate's description; bash just the words.
        let d = |v: &str, help: &str| if zsh { format!("{v}:{help}") } else { v.to_string() };
        let words = |line: &[&str]| tab(sh, &sock, line).0.into_iter().map(|w| w.trim_end().to_string()).filter(|w| !w.starts_with("--")).collect::<Vec<_>>();
        // A single match ends with the space bash 3 can't add itself.
        let bash3 = !zsh && Command::new(sh).args(["-c", "echo ${BASH_VERSINFO[0]}"]).output().unwrap().stdout.starts_with(b"3");
        assert_eq!(
            tab(sh, &sock, &["wh", "show", "D-14"]).0,
            [if zsh {
                "D-14:Retry the uploads"
            } else if bash3 {
                "D-14 "
            } else {
                "D-14"
            }],
            "{sh}"
        );
        assert_eq!(tab(sh, &sock, &["wh", "attach", "sr"]).0, [if zsh { "src" } else { "src/" }], "{sh}: a folder, so no space after it (zsh adds the slash)");

        let sh_ = words(&["wh", "sh"]);
        assert_eq!(sh_.len(), 2, "{sh}: {sh_:?}");
        assert!(sh_[0] == d("show", "One docket's record") && sh_[1].starts_with(&d("share", "Show the user an app")), "{sh}: {sh_:?}");
        assert_eq!(words(&["wh", "show", ""]), [d("D-14", "Retry the uploads"), d("D-12", "Rate limits")], "{sh}: in flight");
        assert_eq!(words(&["wh", "show", "D-1"]), [d("D-14", "Retry the uploads"), d("D-12", "Rate limits"), d("D-13", "done · Fix the login")], "{sh}");
        assert_eq!(words(&["wh", "send", "--to", ""]), [d("me", "you, in the Inbox"), d("api/planner", "Claude Code on D-14 · working")], "{sh}");
        assert_eq!(words(&["wh", "reply", ""]), [d("41", "from api/planner · Migration order")], "{sh}");
        let choice = if zsh { "Users first" } else { "'Users first'" };
        assert_eq!(words(&["wh", "reply", "41", "--choice", "U"]), [choice], "{sh}: bash gets it quoted");
        assert_eq!(words(&["wh", "review", "post-mode", ""]), [d("ask", "you post it"), d("auto", "the checker posts it"), d("config", "what review.lua says")], "{sh}");
        assert_eq!(words(&["wh", "review", "post", ""]), [d("acme/api#12", "posted · Add retries")], "{sh}");
        assert_eq!(
            words(&["wh", "set", "D-4", "--status", "d"]),
            [d("done", "on evidence, or --anyway --reason"), d("dropped", "")].map(|s| s.trim_end_matches(':').to_string()),
            "{sh}"
        );

        // No hub: what's fixed still completes, and nothing is printed.
        let (got, err) = tab(sh, &down, &["wh", "show", ""]);
        assert!(got.iter().all(|w| w.starts_with("--")), "{sh}: {got:?}");
        assert!(err.is_empty(), "{sh} printed: {err}");
        let (got, err) = tab(sh, &down, &["wh", "rev"]);
        assert!(got.len() == 1 && got[0].starts_with(&d("review", "Reviews: status, on, off")), "{sh}: {got:?}");
        assert!(err.is_empty(), "{sh} printed: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
