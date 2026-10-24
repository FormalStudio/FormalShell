#!/usr/bin/env python3
"""The cartridge table's nine blips, synthesised into
crates/fs-audio/sfx/<name>.wav (48 kHz, mono, 16-bit), which fs-audio
embeds. Deterministic: no noise and no clock, so a rerun rewrites the same
bytes and `git diff` shows a recipe change and nothing else.

    dev/sfx/gen.py [out_dir]

Each sound is a few tones: a waveform swept exponentially from f0 to f1,
under a short linear attack and an exponential fall to -80 dB over its
length, the recipe the spec's preview plays through WebAudio. Squares and
saws pass a one-pole low-pass so their top end stays soft. One scale over
all nine puts the loudest at PEAK and keeps the recipe's gains between
them. Prints each sound's peak and RMS in dBFS.
"""

import math
import os
import struct
import sys
import wave

RATE = 48000
PEAK = 0.9
ATTACK = 0.003
TAIL = 0.02
LOWPASS_HZ = 6000.0

# name: [(wave, f0, f1, seconds, gain, at)]
SOUNDS = {
    "move": [("square", 1180, 1500, 0.04, 0.05, 0.0)],
    "activate": [("triangle", 660, 660, 0.06, 0.12, 0.0), ("triangle", 990, 990, 0.09, 0.12, 0.06)],
    "back": [("square", 900, 380, 0.09, 0.05, 0.0)],
    "open": [("triangle", 300, 1200, 0.12, 0.08, 0.0)],
    "close": [("triangle", 1200, 300, 0.12, 0.08, 0.0)],
    "type": [("square", 2400, 2000, 0.015, 0.03, 0.0)],
    "toggle": [("square", 880, 880, 0.03, 0.05, 0.0), ("square", 1320, 1320, 0.04, 0.05, 0.045)],
    "error": [("sawtooth", 140, 110, 0.18, 0.07, 0.0), ("sawtooth", 147, 100, 0.18, 0.05, 0.02)],
    "boot": [("triangle", f, f, 0.14, 0.09, i * 0.09) for i, f in enumerate((440, 554, 659, 880))],
}


def shape(kind, phase):
    p = phase % 1.0
    if kind == "square":
        return 1.0 if p < 0.5 else -1.0
    if kind == "sawtooth":
        return 2.0 * p - 1.0
    return 1.0 - 4.0 * abs(p - 0.5)


def tone(kind, f0, f1, dur, gain):
    n = int(dur * RATE)
    out = [0.0] * n
    phase = 0.0
    fall = math.log(1e-4) / dur
    a = 1.0 - math.exp(-2.0 * math.pi * LOWPASS_HZ / RATE)
    lp = 0.0
    for i in range(n):
        t = i / RATE
        f = f0 * (f1 / f0) ** (t / dur)
        phase += f / RATE
        s = shape(kind, phase)
        if kind != "triangle":
            lp += a * (s - lp)
            s = lp
        env = min(1.0, t / ATTACK) * math.exp(fall * t)
        out[i] = s * env * gain
    return out


def render(parts):
    length = max(at + dur for _, _, _, dur, _, at in parts) + TAIL
    mix = [0.0] * int(length * RATE)
    for kind, f0, f1, dur, gain, at in parts:
        start = int(at * RATE)
        for i, s in enumerate(tone(kind, f0, f1, dur, gain)):
            mix[start + i] += s
    return mix


def db(x):
    return 20.0 * math.log10(x) if x > 0 else float("-inf")


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "..", "crates", "fs-audio", "sfx")
    os.makedirs(out_dir, exist_ok=True)
    mixes = {name: render(parts) for name, parts in SOUNDS.items()}
    scale = PEAK / max(max(abs(s) for s in m) for m in mixes.values())
    for name, mix in mixes.items():
        samples = [max(-32767, min(32767, round(s * scale * 32767))) for s in mix]
        with wave.open(os.path.join(out_dir, f"{name}.wav"), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(struct.pack(f"<{len(samples)}h", *samples))
        peak = max(abs(s) for s in samples) / 32767
        rms = math.sqrt(sum(s * s for s in samples) / len(samples)) / 32767
        print(f"{name:9} {len(samples) / RATE * 1000:5.0f}ms peak {db(peak):6.1f} dBFS rms {db(rms):6.1f} dBFS")


if __name__ == "__main__":
    main()
