#!/usr/bin/env python3
"""Builds the footstep sounds in crates/frontline/assets/sounds/footstep_concrete_000..007.ogg.

Each variant starts from one of Kenney's five concrete recordings (Impact Sounds 1.0, CC0, read from
content/assets/Kenney Impact Sounds.zip), shifts its pitch a little, tilts it towards the highs and
layers two synthesized parts so a step cuts through gunfire and room noise: a short, bright heel
click (high-passed noise with a fast decay) and a low thump (a decaying sine). Every variant gets
its own pitch, click and thump, so a run of steps doesn't repeat. Mono, 48 kHz, Ogg Vorbis
(`oggenc -q 4`), 0.3 s, peak about -1.5 dB.

Pure Python plus ffmpeg and oggenc; no third-party packages.

    python3 tools/audio/footsteps.py            write the eight files
    python3 tools/audio/footsteps.py --report   print what each would measure, write nothing
"""
import array
import math
import random
import subprocess
import sys
import tempfile
import wave
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE_ZIP = ROOT / "content/assets/Kenney Impact Sounds.zip"
OUT_DIR = ROOT / "crates/frontline/assets/sounds"
RATE = 48000
LENGTH_S = 0.30
FADE_S = 0.07
PEAK = 0.84  # about -1.5 dB
VARIANTS = 8


def decode(data: bytes) -> list[float]:
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "in.ogg"
        src.write_bytes(data)
        raw = subprocess.run(
            ["ffmpeg", "-v", "error", "-i", str(src), "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
            check=True, capture_output=True,
        ).stdout
    samples = array.array("h")
    samples.frombytes(raw)
    return [s / 32768.0 for s in samples]


def resample(x: list[float], ratio: float) -> list[float]:
    """Plays `x` `ratio` times faster (higher): linear interpolation."""
    n = int(len(x) / ratio)
    out = []
    for i in range(n):
        pos = i * ratio
        j = int(pos)
        frac = pos - j
        a = x[j]
        b = x[j + 1] if j + 1 < len(x) else 0.0
        out.append(a + (b - a) * frac)
    return out


def one_pole_low(x: list[float], cutoff_hz: float) -> list[float]:
    a = 1.0 - math.exp(-2.0 * math.pi * cutoff_hz / RATE)
    out, y = [], 0.0
    for s in x:
        y += a * (s - y)
        out.append(y)
    return out


def high_pass(x: list[float], cutoff_hz: float) -> list[float]:
    low = one_pole_low(x, cutoff_hz)
    return [s - l for s, l in zip(x, low)]


def onset(x: list[float]) -> int:
    """Index where the step starts: the first sample above 20% of the first 80 ms's peak."""
    window = x[: int(0.08 * RATE)]
    peak = max(abs(s) for s in window) or 1.0
    for i, s in enumerate(window):
        if abs(s) >= 0.2 * peak:
            return i
    return 0


def build(base: list[float], seed: int) -> list[float]:
    rng = random.Random(seed)
    x = resample(base, rng.uniform(0.92, 1.1))
    # Tilt towards the highs: the part above ~1.2 kHz is where footsteps stay audible.
    high = high_pass(x, 1200.0)
    out = [0.5 * s + 2.2 * h for s, h in zip(x, high)]
    out += [0.0] * (int(LENGTH_S * RATE) - len(out))
    start = max(onset(out) - int(0.001 * RATE), 0)
    peak = max(abs(s) for s in out) or 1.0
    # The heel click: bright noise, gone in a few milliseconds.
    click_len = int(0.03 * RATE)
    noise = high_pass([rng.gauss(0.0, 1.0) for _ in range(click_len)], rng.uniform(2200.0, 3200.0))
    tau = rng.uniform(0.0025, 0.0045)
    amp = peak * rng.uniform(1.1, 1.6)
    toe = start + int(rng.uniform(0.09, 0.14) * RATE)  # the forefoot lands a beat later, softer
    for i in range(click_len):
        decay = math.exp(-i / RATE / tau)
        for at, gain in ((start, 1.0), (toe, 0.45)):
            if at + i < len(out):
                out[at + i] += amp * noise[i] * decay * gain
    # The thump: a low sine that dies over about 45 ms.
    freq = rng.uniform(70.0, 115.0)
    thump = peak * rng.uniform(0.35, 0.6)
    thump_tau = rng.uniform(0.03, 0.05)
    for i in range(int(0.15 * RATE)):
        if start + i < len(out):
            out[start + i] += thump * math.sin(2.0 * math.pi * freq * i / RATE) * math.exp(-i / RATE / thump_tau)
    out = out[: int(LENGTH_S * RATE)]
    fade = int(FADE_S * RATE)
    for i in range(fade):
        out[len(out) - fade + i] *= 1.0 - i / fade
    top = max(abs(s) for s in out) or 1.0
    return [s * PEAK / top for s in out]


def high_share(x: list[float]) -> float:
    """Share of the energy above ~2 kHz: how much of a step is the part that carries."""
    low = one_pole_low(x, 2000.0)
    high = [s - l for s, l in zip(x, low)]
    total = sum(s * s for s in x) or 1.0
    return sum(s * s for s in high) / total


def encode(x: list[float], path: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        wav = Path(tmp) / "step.wav"
        with wave.open(str(wav), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(array.array("h", [int(max(-1.0, min(1.0, s)) * 32767) for s in x]).tobytes())
        subprocess.run(["oggenc", "-q", "4", "-Q", "-o", str(path), str(wav)], check=True)


def main() -> None:
    report = "--report" in sys.argv
    with zipfile.ZipFile(SOURCE_ZIP) as zf:
        bases = [decode(zf.read(f"Audio/footstep_concrete_{i:03d}.ogg")) for i in range(5)]
    for v in range(VARIANTS):
        base = bases[v % 5]
        step = build(base, seed=v + 1)
        print(
            f"variant {v}: base {v % 5}, high share {high_share(base):.2f} -> {high_share(step):.2f}, "
            f"{len(step) / RATE:.2f} s"
        )
        if not report:
            encode(step, OUT_DIR / f"footstep_concrete_{v:03d}.ogg")


if __name__ == "__main__":
    main()
