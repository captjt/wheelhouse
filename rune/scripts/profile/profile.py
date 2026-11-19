"""Profile the Rune app and the SwiftUI app the same way, one at a time, on
the same hub data: launch, memory, idle, scrolling, and a stream of changes.

    python3 rune/scripts/profile/profile.py OUT_DIR [--quick]

Needs, already running: the hubs behind proxy.py on 7601 (Wheelhouse Dev's
copy of your dockets, read only) and 7461 (a made-up world with 500
dockets, which this writes notes to), wintool built at .tmp/wintool, Rune's
shipping build (cargo build --profile ship --no-default-features
--target-dir target/ship) and the SwiftUI release binary in an id-less
bundle at .tmp/profile/Wheelhouse.app (notifications off, as Rune's are).
Each app's window comes forward and is placed at the same frame.
"""
import json
import os
import re
import signal
import statistics
import subprocess
import sys
import threading
import time
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
import rusage  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../.."))
OUT = os.path.abspath(sys.argv[1])
QUICK = "--quick" in sys.argv
WINTOOL = os.path.join(ROOT, ".tmp/wintool")
FRAME = (384, 120, 1440, 900)
SCROLL_AT = (FRAME[0] + 420, FRAME[1] + 480)  # over the list column, in both apps
DOCKET_AT = (FRAME[0] + 900, FRAME[1] + 480)  # over the docket's page

APPS = {
    "rune": dict(cmd=[os.path.join(ROOT, "rune/target/ship/ship/wheelhouse-rune")], hub="WH_HUB", env={"WH_RUNE_NOTIFY": "off"}),
    "swiftui": dict(cmd=[os.path.join(ROOT, ".tmp/profile/Wheelhouse.app/Contents/MacOS/Wheelhouse"), "-ApplePersistenceIgnoreState", "YES"], hub="WH_APP_HUB", env={}),
    # The same SwiftUI app without a docket's note field (a throwaway build
    # with an env switch): that NSTextView alone keeps WindowServer busy.
    "swiftui-no-note": dict(cmd=[os.path.join(ROOT, ".tmp/profile/WheelhouseExp.app/Contents/MacOS/Wheelhouse"), "-ApplePersistenceIgnoreState", "YES"], hub="WH_APP_HUB", env={"WH_EXP_NO_COMPOSER": "1"}),
}
MAIN = ["rune", "swiftui"]
# Notes go to a docket that's never on screen, so every session sees the
# same data; D-1, which the Inbox shows, is seeded with its own history.
DATASETS = {
    "yours": dict(port=7601, log=os.path.join(ROOT, ".tmp/profile/proxy-yours.log"), writes=False),
    "large": dict(port=7463, log=os.path.join(ROOT, ".tmp/profile/proxy-clean.log"), writes=True, hub=7462, target="D-400"),
    "extreme": dict(port=7461, log=os.path.join(ROOT, ".tmp/profile/proxy-large.log"), writes=False, short=True),
}
ONLY = [a.split("=", 1)[1].split(",") for a in sys.argv if a.startswith("--datasets=")]
if ONLY:
    DATASETS = {k: v for k, v in DATASETS.items() if k in ONLY[0]}
LAUNCHES = 2 if QUICK else 5
SESSIONS = 1 if QUICK else 2
PHASES = dict(settle=15, inbox=45, list_settle=8, list=30, scroll=20, scroll_settle=5, active=30, updates=60, after=30)
if QUICK:
    PHASES = {k: max(3, v // 5) for k, v in PHASES.items()}

WS_PID = int(subprocess.run(["pgrep", "-x", "WindowServer"], capture_output=True, text=True).stdout.split()[0])


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def wintool(*args):
    return subprocess.run([WINTOOL, *map(str, args)], capture_output=True, text=True).stdout.strip()


def spawn(app, ds):
    a = APPS[app]
    env = dict(os.environ, **a["env"], **{a["hub"]: f"http://127.0.0.1:{DATASETS[ds]['port']}"})
    return subprocess.Popen(a["cmd"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def stop(p):
    p.send_signal(signal.SIGTERM)
    try:
        p.wait(timeout=8)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait()


def requests_since(ds, t0, t1=None):
    out = []
    with open(DATASETS[ds]["log"]) as f:
        for line in f:
            t, method, path = line.split(" ", 2)
            t = int(t)
            if t >= t0 and (t1 is None or t <= t1):
                out.append((t, method, path.strip()))
    return out


PAGE = re.compile(r"^/api/dockets/[^/?]+$")


class Phase:
    """A stretch of time: the app's counters and WindowServer's CPU across it."""

    def __init__(self, pid):
        self.pid = pid
        self.t = time.time_ns()
        self.r = rusage.sample(pid)
        self.ws = rusage.ps_cpu_s(WS_PID)

    def end(self, name, ds):
        t = time.time_ns()
        r = rusage.sample(self.pid)
        ws = rusage.ps_cpu_s(WS_PID)
        secs = (t - self.t) / 1e9
        reqs = requests_since(ds, self.t, t)
        return {
            "phase": name,
            "secs": round(secs, 2),
            "cpu_pct": 100 * (r["cpu_ns"] - self.r["cpu_ns"]) / 1e9 / secs,
            "ws_cpu_pct": 100 * (ws - self.ws) / secs,
            "wakeups_per_s": (r["wakeups"] - self.r["wakeups"]) / secs,
            "instructions_per_s": (r["instructions"] - self.r["instructions"]) / secs,
            "footprint_mb": r["footprint"] / 2**20,
            "max_footprint_mb": r["max_footprint"] / 2**20,
            "state_fetches": sum(1 for _, m, p in reqs if m == "GET" and p.startswith("/api/state")),
            "page_fetches": sum(1 for _, m, p in reqs if m == "GET" and PAGE.match(p)),
        }


def baseline(secs):
    t, ws = time.time(), rusage.ps_cpu_s(WS_PID)
    time.sleep(secs)
    return 100 * (rusage.ps_cpu_s(WS_PID) - ws) / (time.time() - t)


def launch(app, ds):
    t0 = time.time_ns()
    p = spawn(app, ds)
    tw = wintool("wait", p.pid, 30)
    wintool("front", p.pid)
    time.sleep(max(0, 8 - (time.time_ns() - t0) / 1e9))
    r = rusage.sample(p.pid)
    reqs = requests_since(ds, t0)
    stop(p)

    def first(pred):
        ts = [t for t, m, path in reqs if pred(m, path)]
        return round((min(ts) - t0) / 1e6, 1) if ts else None

    return {
        "app": app,
        "dataset": ds,
        "window_ms": round((int(tw) - t0) / 1e6, 1) if tw.isdigit() else None,
        "state_ms": first(lambda m, p: m == "GET" and p.startswith("/api/state")),
        "events_ms": first(lambda m, p: m == "GET" and p.startswith("/api/events")),
        "page_ms": first(lambda m, p: m == "GET" and bool(PAGE.match(p))),
        "startup_cpu_ms": round(r["cpu_ns"] / 1e6, 1),
        "footprint_mb": round(r["footprint"] / 2**20, 1),
        "instructions_m": round(r["instructions"] / 1e6, 1),
    }


def notes(stop_at, ds):
    """Notes into the made-up world, ten a second: an agent at work."""
    base = f"http://127.0.0.1:{DATASETS[ds]['hub']}"
    i = 0
    while time.time() < stop_at:
        body = json.dumps({"body": f"Progress {i}: ran the suite, 3 left."}).encode()
        req = urllib.request.Request(f"{base}/api/dockets/{DATASETS[ds]['target']}/note", data=body, headers={"Content-Type": "application/json", "X-Wheelhouse": "profile"}, method="POST")
        try:
            urllib.request.urlopen(req, timeout=5).read()
        except Exception as e:  # noqa: BLE001
            log("note failed", e)
        i += 1
        time.sleep(0.1)
    return i


def session(app, ds):
    P = PHASES
    p = spawn(app, ds)
    wintool("wait", p.pid, 30)
    wintool("front", p.pid)
    wintool("frame", p.pid, *FRAME)
    out = []
    time.sleep(P["settle"])
    ph = Phase(p.pid)
    time.sleep(P["inbox"])
    out.append(ph.end("inbox idle", ds))
    wintool("front", p.pid)
    ph = Phase(p.pid)
    wintool("scroll", p.pid, *DOCKET_AT, P["scroll"], 60, 40)
    out.append(ph.end("scrolling a docket", ds))
    time.sleep(P["scroll_settle"])
    if DATASETS[ds].get("short"):
        stop(p)
        for r in out:
            r.update(app=app, dataset=ds)
        return out
    wintool("key", p.pid, "cmd+5")  # All dockets, in both
    time.sleep(P["list_settle"])
    ph = Phase(p.pid)
    time.sleep(P["list"])
    out.append(ph.end("all dockets idle", ds))
    wintool("front", p.pid)
    ph = Phase(p.pid)
    wintool("scroll", p.pid, *SCROLL_AT, P["scroll"], 60, 40)
    out.append(ph.end("scrolling the list", ds))
    time.sleep(P["scroll_settle"])
    wintool("key", p.pid, "cmd+4")  # Active: on the made-up world, an agent at work in view
    time.sleep(P["list_settle"])
    ph = Phase(p.pid)
    time.sleep(P["active"])
    out.append(ph.end("active idle", ds))
    if DATASETS[ds]["writes"]:
        ph = Phase(p.pid)
        n = notes(time.time() + P["updates"], ds)
        r = ph.end("10 changes a second", ds)
        r["changes"] = n
        out.append(r)
    ph = Phase(p.pid)
    time.sleep(P["after"])
    out.append(ph.end("after, idle", ds))
    stop(p)
    for r in out:
        r.update(app=app, dataset=ds)
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    results = {"started": time.strftime("%Y-%m-%d %H:%M:%S"), "phases": PHASES, "launches": [], "sessions": [], "baseline_ws_cpu_pct": []}
    save = lambda: json.dump(results, open(os.path.join(OUT, "results.json"), "w"), indent=1)  # noqa: E731
    log("baseline: WindowServer with neither app")
    results["baseline_ws_cpu_pct"].append(baseline(10 if QUICK else 30))
    for ds in [d for d in DATASETS if not DATASETS[d].get("short")]:
        for i in range(LAUNCHES):
            for app in (MAIN if i % 2 == 0 else MAIN[::-1]):
                r = launch(app, ds)
                log("launch", r)
                results["launches"].append(r)
                save()
                time.sleep(2)
    for ds in DATASETS:
        for i in range(SESSIONS):
            for app in (MAIN if i % 2 == 0 else MAIN[::-1]) + (["swiftui-no-note"] if i == 0 else []):
                log("session", app, ds)
                rs = session(app, ds)
                for r in rs:
                    log("  ", r["phase"], f"cpu {r['cpu_pct']:.1f}%", f"ws {r['ws_cpu_pct']:.1f}%", f"{r['footprint_mb']:.0f} MB")
                results["sessions"] += rs
                save()
                time.sleep(3)
    results["baseline_ws_cpu_pct"].append(baseline(10 if QUICK else 30))
    results["finished"] = time.strftime("%Y-%m-%d %H:%M:%S")
    save()
    summary(results)


def summary(res):
    med = lambda xs: statistics.median([x for x in xs if x is not None]) if any(x is not None for x in xs) else None  # noqa: E731
    lines = ["# Launch (median of %d)" % LAUNCHES]
    for ds in DATASETS:
        for app in MAIN:
            rs = [r for r in res["launches"] if r["app"] == app and r["dataset"] == ds]
            lines.append(f"{ds:6} {app:8} window {med([r['window_ms'] for r in rs])} ms · state {med([r['state_ms'] for r in rs])} ms · page {med([r['page_ms'] for r in rs])} ms · startup CPU {med([r['startup_cpu_ms'] for r in rs])} ms · {med([r['footprint_mb'] for r in rs])} MB")
    base = statistics.mean(res["baseline_ws_cpu_pct"])
    lines.append(f"\n# Sessions (median of {SESSIONS}); WindowServer baseline {base:.1f}%")
    for ds in DATASETS:
        for ph in dict.fromkeys(r["phase"] for r in res["sessions"] if r["dataset"] == ds):
            for app in APPS:
                rs = [r for r in res["sessions"] if r["app"] == app and r["dataset"] == ds and r["phase"] == ph]
                if rs:
                    lines.append(
                        f"{ds:6} {ph:20} {app:16} cpu {med([r['cpu_pct'] for r in rs]):5.1f}% · WindowServer +{med([r['ws_cpu_pct'] for r in rs]) - base:5.1f}% · {med([r['wakeups_per_s'] for r in rs]):6.0f} wakeups/s · {med([r['footprint_mb'] for r in rs]):5.0f} MB · fetches {med([r['state_fetches'] for r in rs])}"
                    )
    text = "\n".join(lines)
    open(os.path.join(OUT, "summary.txt"), "w").write(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
