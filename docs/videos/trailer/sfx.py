"""Score the trailer: soft plucks, chords and ticks on the page's cues.

    python3 sfx.py cues.json out.wav

cues.json is what make.js takes from trailer.html: {"total": ms, "cues":
[{"t": ms, "kind": ..., ...}]}. Everything is synthesised here (numpy), so
the sound is ours and the same on every run.
"""
import json, sys, wave
import numpy as np

SR = 48000
rng = np.random.default_rng(7)

cfg = json.load(open(sys.argv[1]))
TOTAL = cfg["total"] / 1000
N = int((TOTAL + 0.2) * SR)
dry = np.zeros((N, 2))
pad = np.zeros((N, 2))


def hz(note):
    """'D4', 'F#5' → Hz."""
    names = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
    n = names[note[0]]
    rest = note[1:]
    if rest[0] == "#":
        n += 1
        rest = rest[1:]
    elif rest[0] == "b":
        n -= 1
        rest = rest[1:]
    midi = 12 * (int(rest) + 1) + n
    return 440.0 * 2 ** ((midi - 69) / 12)


def put(buf, t, sig, pan=0.0):
    i = int(t * SR)
    if i >= len(buf):
        return
    sig = sig[: len(buf) - i]
    l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    buf[i : i + len(sig), 0] += sig * l
    buf[i : i + len(sig), 1] += sig * r


def env(n, attack, tau):
    t = np.arange(n) / SR
    a = np.clip(t / attack, 0, 1) if attack > 0 else 1
    return a * np.exp(-t / tau)


def pluck(f, amp=0.2, tau=0.45, bright=0.35, dur=1.6):
    """A soft mallet: the note, a quick octave and a touch of bell."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    s = np.sin(2 * np.pi * f * t) * env(n, 0.004, tau)
    s += bright * np.sin(2 * np.pi * 2 * f * t) * env(n, 0.002, tau / 4)
    s += bright * 0.3 * np.sin(2 * np.pi * 2.76 * f * t) * env(n, 0.001, tau / 7)
    return amp * s


def tick(amp=0.05):
    """A key: a short, muted tock with a little click."""
    n = int(0.05 * SR)
    t = np.arange(n) / SR
    f = rng.uniform(1500, 2100)
    s = np.sin(2 * np.pi * f * t) * env(n, 0.0005, 0.006)
    s += 0.5 * rng.standard_normal(n) * env(n, 0, 0.0015)
    s += 0.6 * np.sin(2 * np.pi * 220 * t) * env(n, 0.001, 0.01)
    return amp * s


def sweep(f0, f1, dur, amp):
    n = int(dur * SR)
    t = np.arange(n) / SR
    f = np.geomspace(f0, f1, n)
    ph = 2 * np.pi * np.cumsum(f) / SR
    return amp * np.sin(ph) * np.sin(np.pi * np.clip(t / dur, 0, 1)) ** 2


def whoosh(dur, amp, rising):
    """Air: noise through a band that opens (or closes) with the move."""
    n = int(dur * SR)
    noise = rng.standard_normal(n)
    spec = np.fft.rfft(noise)
    fr = np.fft.rfftfreq(n, 1 / SR)
    spec *= np.exp(-((np.log(fr + 1) - np.log(900)) ** 2) / 1.2)
    s = np.fft.irfft(spec, n)
    s /= np.abs(s).max() + 1e-9
    x = np.linspace(0, 1, n)
    shape = np.sin(np.pi * x) ** 2 * (0.6 + 0.4 * (x if rising else 1 - x))
    return amp * s * shape


def strum(notes, t, amp, gap=0.03, tau=0.9, buf=None, bright=0.3):
    for k, nm in enumerate(notes):
        put(buf if buf is not None else dry, t + k * gap, pluck(hz(nm), amp, tau, bright, 2.6), pan=(k / max(1, len(notes) - 1) - 0.5) * 0.6)


# One key, D major; each take gets a chord of its own.
CHORDS = [
    ["D3", "A3", "C#4", "E4", "F#4"],   # capture: Dmaj9
    ["B2", "F#3", "A3", "D4", "E4"],    # today: Bm11
    ["G2", "D3", "F#3", "A3", "B3"],    # español: Gmaj9
    ["A2", "E3", "F#3", "C#4", "E4"],   # week: A6
    ["F#2", "C#3", "E3", "A3", "C#4"],  # month: F#m7
    ["G2", "D3", "F#3", "B3", "C#4"],   # calendars: Gmaj7#11
    ["E2", "B2", "D3", "F#3", "G3"],    # themes: Em9
]
OUTRO = ["D2", "A2", "F#3", "A3", "C#4", "E4"]
SCALE = ["D5", "E5", "F#5", "A5", "B5", "D6", "E6"]


def pad_chord(notes, t0, t1, amp=0.009):
    """A warm bed under a take: detuned sines, slow in, slow out."""
    if t1 <= t0:
        return
    n = int((t1 - t0 + 1.2) * SR)
    t = np.arange(n) / SR
    s = np.zeros((n, 2))
    for nm in notes:
        f = hz(nm)
        for ch, det in ((0, -0.12), (1, 0.12)):
            v = np.sin(2 * np.pi * (f + det) * t + rng.uniform(0, 6.28))
            v += 0.18 * np.sin(2 * np.pi * 2 * (f + det) * t)
            s[:, ch] += v
    a = np.clip(t / 0.5, 0, 1) * np.clip((t1 - t0 + 1.2 - t) / 1.2, 0, 1)
    lfo = 1 + 0.08 * np.sin(2 * np.pi * 0.3 * t)
    s *= (a * lfo)[:, None] * amp
    i = int(t0 * SR)
    m = min(n, N - i)
    pad[i : i + m] += s[:m]


cues = sorted(cfg["cues"], key=lambda c: c["t"])
cuts = [c for c in cues if c["kind"] == "cut"]
outro_t = next(c["t"] for c in cues if c["kind"] == "outro") / 1000
for k, c in enumerate(cuts):
    t0 = c["t"] / 1000
    t1 = cuts[k + 1]["t"] / 1000 if k + 1 < len(cuts) else outro_t
    pad_chord(CHORDS[c["i"] % len(CHORDS)], max(0, t0 - 0.15), t1)

month_step = 0
theme_step = 0
for c in cues:
    t = c["t"] / 1000
    kind = c["kind"]
    if kind == "letter":
        put(dry, t, pluck(hz(["D5", "F#5", "A5", "C#6"][c["i"]]), 0.16, 0.5, 0.4), pan=-0.3 + 0.2 * c["i"])
    elif kind == "tag":
        strum(["D4", "A4", "E5"], t, 0.09, gap=0.05, tau=1.2)
    elif kind == "enter":
        put(dry, t, whoosh(0.9, 0.05, True))
        put(dry, t + 0.35, pluck(hz("D3"), 0.14, 1.2, 0.15, 3))
    elif kind == "cut":
        ch = CHORDS[c["i"] % len(CHORDS)]
        if c["i"] > 0:
            put(dry, t - 0.12, whoosh(0.5, 0.03, False))
        strum([ch[0], ch[2], ch[4]], t, 0.07, gap=0.045, tau=1.1)
    elif kind == "zoom":
        put(dry, t, whoosh(min(1.4, c["len"] / 1000), 0.035, c["dir"] > 0))
    elif kind == "type":
        put(dry, t, tick(0.045 if c.get("arg") != " " else 0.03), pan=rng.uniform(-0.25, 0.25))
    elif kind == "key":
        arg, scene = c.get("arg"), c.get("scene")
        if arg == "n":                      # the capture box opens
            put(dry, t, sweep(420, 840, 0.09, 0.09))
            put(dry, t + 0.03, pluck(hz("A5"), 0.06, 0.25))
        elif arg == "Enter" and scene == "themes":
            ch = [["E4", "G4", "B4", "D5"], ["F#4", "A4", "C#5", "E5"], ["G4", "B4", "D5", "F#5"]][theme_step % 3]
            theme_step += 1
            strum(ch, t, 0.07, gap=0.035, tau=0.8, bright=0.45)
        elif arg == "Enter":                 # a task saved
            put(dry, t, pluck(hz("A5"), 0.13, 0.5, 0.5))
            put(dry, t + 0.08, pluck(hz("D6"), 0.13, 0.8, 0.5))
        elif arg == "Escape":
            put(dry, t, sweep(700, 380, 0.08, 0.05))
        elif arg == "j" and scene == "month":
            put(dry, t, pluck(hz(SCALE[::-1][month_step % len(SCALE)]), 0.07, 0.3, 0.3))
            month_step += 1
        elif arg == "v":
            put(dry, t, sweep(500, 900, 0.08, 0.06))
        elif arg in ("T", "j"):
            pass
        else:
            put(dry, t, tick(0.05))
    elif kind == "outro":
        put(dry, t - 0.3, whoosh(0.8, 0.04, True))
        strum(OUTRO, t, 0.09, gap=0.06, tau=2.2, bright=0.25)
        put(dry, t + 0.7, pluck(hz("D6"), 0.08, 1.6, 0.4, 3.5))
        put(dry, t + 1.0, pluck(hz("A5"), 0.05, 1.6, 0.4, 3.5))
        pad_chord(OUTRO, t, TOTAL - 1.0, amp=0.012)


def reverb(x, secs=2.2, mix=0.28):
    """A room: convolve with two decorrelated decaying noise tails."""
    n = int(secs * SR)
    tt = np.arange(n) / SR
    out = np.zeros_like(x)
    size = 1 << int(np.ceil(np.log2(len(x) + n)))
    for ch in range(2):
        ir = rng.standard_normal(n) * np.exp(-tt / (secs / 6.9))
        ir = np.convolve(ir, np.ones(24) / 24, mode="same")  # darker tail
        ir[: int(0.012 * SR)] = 0
        ir /= np.sqrt((ir ** 2).sum())
        y = np.fft.irfft(np.fft.rfft(x[:, ch], size) * np.fft.rfft(ir, size), size)[: len(x)]
        out[:, ch] = y
    return x * (1 - mix) + out * mix * 1.6


mixed = reverb(dry) + reverb(pad, 3.0, 0.4)
# Gentle glue, then a fade out at the very end.
mixed = np.tanh(mixed * 1.4) / 1.4
fade = np.clip((TOTAL - np.arange(N) / SR) / 1.2, 0, 1)
mixed *= fade[:, None]
mixed /= np.abs(mixed).max() + 1e-9
mixed *= 10 ** (-1.5 / 20)

pcm = (mixed * 32767).astype("<i2")
with wave.open(sys.argv[2], "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(pcm.tobytes())
print("sfx", len(cues), "cues", round(TOTAL, 1), "s")
