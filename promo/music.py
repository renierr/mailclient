"""Audible chill synth bed for the promo. Stdlib only. 60s stereo WAV.
Chords: Am F C G x2, 7.5s each. Pad + bass + plucked arpeggio.
Prints peak/RMS dB so loudness is VERIFIED, not assumed."""
import math, struct, wave, sys

SR = 44100
CHORDS = [  # (bass, pad tones, arp tones)
    (55.00, [110.00, 130.81, 164.81, 220.00], [220.00, 261.63, 329.63, 440.00]),  # Am
    (87.31, [87.31, 110.00, 130.81, 174.61], [174.61, 220.00, 261.63, 349.23]),  # F
    (65.41, [130.81, 164.81, 196.00, 261.63], [261.63, 329.63, 392.00, 523.25]), # C
    (49.00, [98.00, 123.47, 146.83, 196.00], [196.00, 246.94, 293.66, 392.00]),  # G
] * 2
CHORD_DUR = 7.5
TOTAL = CHORD_DUR * len(CHORDS)
N = int(SR * TOTAL)

def env(t, dur, attack, release):
    a = min(1.0, t / attack) if attack > 0 else 1.0
    a = a * a * (3 - 2 * a)  # smoothstep
    r = min(1.0, (dur - t) / release) if release > 0 else 1.0
    r = r * r * (3 - 2 * r)
    return min(a, r)

buf = [0.0] * N
for ci, (bass, pad, arp) in enumerate(CHORDS):
    start = int(ci * CHORD_DUR * SR)
    m = int(CHORD_DUR * SR)
    for i in range(m):
        t = i / SR
        gi = start + i
        if gi >= N: break
        e = env(t, CHORD_DUR, 2.0, 2.5)
        lfo = 1 + 0.18 * math.sin(2 * math.pi * 0.1 * (t + ci * CHORD_DUR))
        for fq in pad:  # warm pad
            buf[gi] += 0.085 * e * lfo * math.sin(2 * math.pi * fq * t)
        buf[gi] += 0.20 * env(t, CHORD_DUR, 0.3, 1.0) * math.sin(2 * math.pi * bass * t)  # bass
        buf[gi] += 0.04 * env(t, CHORD_DUR, 0.3, 1.0) * math.sin(4 * math.pi * bass * t)
    step = 0.3  # plucked arpeggio
    k = 0
    st = 0.0
    while st < CHORD_DUR - 0.3:
        fq = arp[k % len(arp)]
        s0 = int(start + st * SR)
        ln = int(0.6 * SR)
        for i in range(ln):
            gi = s0 + i
            if gi >= N: break
            t = i / SR
            d = math.exp(-t / 0.22)
            buf[gi] += 0.16 * d * math.sin(2 * math.pi * fq * t)
            buf[gi] += 0.05 * math.exp(-t / 0.10) * math.sin(4 * math.pi * fq * t)
        st += step; k += 1

# master fade + normalize to peak 0.5 (-6 dBFS)
fade = int(2.5 * SR)
for i in range(fade):
    buf[i] *= i / fade
    buf[N - 1 - i] *= i / fade
peak = max(abs(v) for v in buf)
g = 0.5 / peak
buf = [v * g for v in buf]
peak = max(abs(v) for v in buf)
rms = math.sqrt(sum(v * v for v in buf) / N)
print(f'peak_dB={20 * math.log10(peak):.1f} rms_dB={20 * math.log10(rms):.1f}', flush=True)
assert rms > 0.05, 'MUSIC TOO QUIET - refusing to write'

out = sys.argv[1]
with wave.open(out, 'wb') as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes(b''.join(struct.pack('<hh', int(max(-1, min(1, v)) * 32767), int(max(-1, min(1, v)) * 32767)) for v in buf))
print('wrote', out, flush=True)
