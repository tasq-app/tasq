"""Record tasq scenes as terminal frames on a virtual timeline.

Each step advances virtual time by its duration; a frame is captured after
the app has settled, so video timing doesn't depend on how fast this box is.
"""
import json, subprocess, time, hashlib, os, sys

OUT = os.path.dirname(os.path.abspath(__file__)) + "/frames"
os.makedirs(OUT, exist_ok=True)
COLS, ROWS = 120, 34

def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout

def start():
    subprocess.run(["tmux", "kill-server"], capture_output=True)
    time.sleep(0.2)
    here = os.path.dirname(os.path.abspath(__file__))
    subprocess.run(["tmux", "new-session", "-d", "-s", "t", "-x", str(COLS), "-y", str(ROWS),
                    f"bash -c 'source {here}/seed.sh; $TASQ_BIN; sleep 600'"])
    time.sleep(2.5)

def keys(*k):
    subprocess.run(["tmux", "send-keys", "-t", "t", *k])

def lit(s):
    subprocess.run(["tmux", "send-keys", "-t", "t", "-l", s])

def grab(settle=0.18):
    time.sleep(settle)
    return sh("tmux", "capture-pane", "-e", "-N", "-t", "t", "-p")

def record(name, pre, steps):
    """pre: keys sent before recording. steps: (kind, arg, ms)."""
    start()
    for k in pre:
        if k.startswith("@"):
            time.sleep(int(k[1:]) / 1000)
        elif k.startswith("lit:"):
            lit(k[4:]); time.sleep(0.25)
        else:
            keys(k); time.sleep(0.3)
    t = 0
    frames = []
    def snap(settle=0.18):
        a = grab(settle)
        h = hashlib.sha1(a.encode()).hexdigest()[:12]
        path = f"{OUT}/{h}.ansi"
        if not os.path.exists(path):
            open(path, "w").write(a)
        if not frames or frames[-1][1] != h:
            frames.append((t, h))
    snap()
    for kind, arg, ms in steps:
        if kind == "type":
            per = ms
            for ch in arg:
                lit(ch); t += per; snap()
        elif kind == "key":
            keys(arg); t += ms; snap(0.9 if arg == "Enter" and self_themes else 0.18)
        elif kind == "settle":
            time.sleep(1.0); snap()
        elif kind == "wait":
            t += ms; time.sleep(ms / 1000 * 0.5); snap()
    subprocess.run(["tmux", "kill-server"], capture_output=True)
    return {"name": name, "frames": frames, "length": t}

SCENES = []
ONLY = sys.argv[1:]
self_themes = "themes" in ONLY
def scene(name, pre, steps):
    if ONLY and name not in ONLY:
        return
    SCENES.append(record(name, pre, steps))
    print(name, len(SCENES[-1]["frames"]), "frames", SCENES[-1]["length"], "ms", file=sys.stderr)

exec(open(os.path.dirname(os.path.abspath(__file__)) + "/script.py").read())
if ONLY:
    old = json.load(open(os.path.dirname(os.path.abspath(__file__)) + "/scenes.json"))
    new = {x["name"]: x for x in SCENES}
    SCENES = [new.get(x["name"], x) for x in old] + [x for x in SCENES if x["name"] not in {o["name"] for o in old}]
json.dump(SCENES, open(os.path.dirname(os.path.abspath(__file__)) + "/scenes.json", "w"), indent=1)
