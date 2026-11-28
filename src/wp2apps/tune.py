"""Shapes music for the watch's small speaker before it is encoded (wp2apps.play).

Measured on a real watch, 2026-11-21 (a phone recording of Crab Rave): the song has most of its energy below
150 Hz, which the speaker cannot move; the amplifier still drives it, the speaker distorts (peaks at full scale,
a 600-1200 Hz hump) and the music sounds muddy. So: cut below ~220 Hz, lift the presence band a little, then
compress and limit so it is loud without clipping. Pure Python biquads (RBJ cookbook), no dependencies."""
import math


def _biquad(x: list[float], b0, b1, b2, a0, a1, a2) -> list[float]:
    b0, b1, b2, a1, a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0
    y, x1, x2, y1, y2 = [], 0.0, 0.0, 0.0, 0.0
    for v in x:
        o = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, o
        y.append(o)
    return y


def highpass(x, rate, hz, q=0.7071):
    w = 2 * math.pi * hz / rate
    c, a = math.cos(w), math.sin(w) / (2 * q)
    return _biquad(x, (1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + a, -2 * c, 1 - a)


def lowpass(x, rate, hz, q=0.7071):
    w = 2 * math.pi * hz / rate
    c, a = math.cos(w), math.sin(w) / (2 * q)
    return _biquad(x, (1 - c) / 2, 1 - c, (1 - c) / 2, 1 + a, -2 * c, 1 - a)


def virtual_bass(x, rate, below=230.0, amount=1.0, land=(380.0, 900.0)):
    """The bass the driver can't play, as harmonics it can: the band under `below` through a soft nonlinearity
    (x|x| for odd harmonics plus |x| for even), band-passed to 2-4x `below`. The ear hears the missing fundamental."""
    b = lowpass(lowpass(x, rate, below), rate, below)
    peak_ = max((abs(v) for v in b), default=1.0) or 1.0
    h = [(v * abs(v) / peak_) + 0.6 * abs(v) for v in b]
    h = highpass(highpass(h, rate, land[0]), rate, land[0])  # above the 200-450 Hz buzz zone (recording 19)
    h = lowpass(lowpass(h, rate, land[1]), rate, land[1])
    return [amount * v for v in h]


def tame(x, rate, lo=900.0, hi=1800.0, over_db=-18.0, ratio=4.0, attack_ms=5.0, release_ms=150.0):
    """Dynamic EQ: when the lo-hi band gets louder than `over_db` under the whole signal, it is turned down
    (ratio:1 over that). Recording 22: a 3 s passage where 1-1.6 kHz jumped 8-13 dB over the song and the
    driver's resonance rang like a cheap radio, while the rest stayed clean."""
    band = lowpass(highpass(highpass(x, rate, lo), rate, lo), rate, hi)
    band = lowpass(band, rate, hi)
    at, rl = math.exp(-1 / (rate * attack_ms / 1000)), math.exp(-1 / (rate * release_ms / 1000))
    eb = ea = 1.0
    k = 10 ** (over_db / 20)
    out = []
    for v, b in zip(x, band):
        for e, s_ in ((0, abs(b)), (1, abs(v))):
            if e == 0:
                eb = (at if s_ > eb else rl) * eb + (1 - (at if s_ > eb else rl)) * s_
            else:
                ea = (at if s_ > ea else rl) * ea + (1 - (at if s_ > ea else rl)) * s_
        lim = k * ea
        g = (lim * (eb / lim) ** (1 / ratio)) / eb if eb > lim > 0 else 1.0
        out.append(v - (1 - g) * b)
    return out


def peak(x, rate, hz, db, q=1.0):
    A, w = 10 ** (db / 40), 2 * math.pi * hz / rate
    c, a = math.cos(w), math.sin(w) / (2 * q)
    return _biquad(x, 1 + a * A, -2 * c, 1 - a * A, 1 + a / A, -2 * c, 1 - a / A)


def high_shelf(x, rate, hz, db, s=0.9):
    A, w = 10 ** (db / 40), 2 * math.pi * hz / rate
    c, sn = math.cos(w), math.sin(w)
    al = sn / 2 * math.sqrt((A + 1 / A) * (1 / s - 1) + 2)
    r = 2 * math.sqrt(A) * al
    return _biquad(x, A * ((A + 1) + (A - 1) * c + r), -2 * A * ((A - 1) + (A + 1) * c), A * ((A + 1) + (A - 1) * c - r),
                   (A + 1) - (A - 1) * c + r, 2 * ((A - 1) - (A + 1) * c), (A + 1) - (A - 1) * c - r)


def compress(x, rate, threshold_db=-24.0, ratio=2.5, attack_ms=10.0, release_ms=200.0, ceiling=0.8):
    """Feed-forward compressor, then make-up gain to the ceiling and a hard-knee peak limiter."""
    at, rl = math.exp(-1 / (rate * attack_ms / 1000)), math.exp(-1 / (rate * release_ms / 1000))
    thr = 10 ** (threshold_db / 20) * 32768
    env, out = 0.0, []
    for v in x:
        a = abs(v)
        env = at * env + (1 - at) * a if a > env else rl * env + (1 - rl) * a
        g = (thr * (env / thr) ** (1 / ratio)) / env if env > thr else 1.0
        out.append(v * g)
    top = max((abs(v) for v in out), default=1.0) or 1.0
    # make-up: the 99.9th percentile peak to the ceiling, the rest is limited
    srt = sorted(abs(v) for v in out[:: max(1, len(out) // 20000)])
    p = srt[int(len(srt) * 0.999)] if srt else top
    k = ceiling * 32767 / max(p, 1.0)
    lim = ceiling * 32767
    return [max(-lim, min(lim, v * k)) for v in out]


# Measured on a real watch, 2026-11-21 (phone and Mac mic recordings): the speaker does play bass (a 300 Hz cut
# made it a tin can), but it adds a hump at 600-1200 Hz that buries the top, and the top rolls off from ~4 kHz.
# So: only a gentle cut below 90 Hz, a moderate dip at the hump, lift 2-5 kHz.
# Then (recording 7): +6 dB of shelf and hard compression was harsh; the watch now filters the upsampling image.
# Recording 8: clean but a phone-call sound (nothing above 5.3 kHz); the watch adds shimmer there now (exciter),
# the shelf comes back up (the harshness was the upsampling image, filtered now) and the bass is trimmed a little.
# Recordings 15 and 16 (a stepped sine sweep, phone 5 cm from the watch, room noise removed), 2026-11-21:
#   Hz  100  140  200  250  320  400  500  640  800 1000 1300 2000 3000 4000
#   dB  -30  -22  -14   -7   -1   +3   +5   +6   +5   +3   +1   -1    0   +2
# and loud tones under ~320 Hz distort (harmonics 10-14 dB under the tone: the driver shakes). So: real bass out
# below 230 Hz, put back as virtual bass (harmonics at 300-800 Hz); a broad dip at the 640 Hz resonance; flat top.
# Recording 18 (a level ramp, 3 dB steps at volume 80): 400/1600/3200 Hz grow cleanly up to -6 dBFS and then the
# output falls (the stock amplifier's protection, likely); 800 Hz stops growing at -18 dBFS (the resonance
# saturates first); 250 Hz collapses into distortion above -15 dBFS. So the peaks stay at -6 dBFS, everything
# under 500 Hz is limited 12 dB lower, and the resonance dip is deeper.
# Recording 19: a slow glide shows no sharp rattle, but a buzz (non-harmonic noise above 2.5 kHz, -20 dB re the
# tone) from 200 to 450 Hz; bass and treble together intermodulate only at -30 dB (fine); pink noise puts the
# hump at 800-1300 Hz (+4 dB over the 1.6-4 kHz plateau). Virtual bass lands at 380-900 Hz, out of the buzz.
# Recordings 13 (the warm tuning, preferred) and 20 (the measured one) against the song, 60 s each: 13 had ~12 dB
# more 100-500 Hz (the body), 20 flat mids but 3-6 kHz 5-7 dB hot. The middle: some real bass back (gentle cut
# at 160 Hz, still limited), body at 420 Hz, a lighter dip at the hump, the top down 4 dB.
# Recording 23: 600 Hz - 5 kHz within 2 dB of the song; a bit more warmth (body, virtual bass) to finish.
TUNING = {"cut_hz": 160.0, "cut_order": 2, "vbass": 0.9, "body_hz": 420.0, "body_db": 6.0, "reso_hz": 900.0,
          "reso_db": -5.0, "reso_q": 0.8, "top_db": -4.0, "top_hz": 2800.0,
          "split_hz": 500.0, "low_ceiling": 0.125, "ceiling": 0.5}


# Loudness levelling: a quietly mastered song is brought up (and a very loud one down) to one level before the
# tuning, so songs sound alike and the compressor works the same on each.
TARGET_DB = -14.0  # gated RMS, dBFS
GAIN_DB = (-6.0, 12.0)


def level(samples: list[int], rate: int) -> float:
    """The gain to TARGET_DB from the song's gated RMS (0.4 s windows; silence under -50 dBFS and windows 10 dB
    under the mean are left out, as loudness meters do). CMFX: SpeakerTune.level, the same."""
    n = int(rate * 0.4)
    powers = []
    for k in range(0, len(samples) - n + 1, n):
        acc = 0.0
        for v in samples[k:k + n]:
            acc += float(v) * float(v)
        powers.append(acc / n)
    floor = (32767.0 * 10 ** (-50 / 20)) ** 2
    powers = [p for p in powers if p > floor]
    if not powers:
        return 1.0
    mean = sum(powers) / len(powers)
    kept = [p for p in powers if p >= mean / 10]
    rms_db = 10 * math.log10(sum(kept) / len(kept) / (32767.0 * 32767.0))
    return 10 ** (max(GAIN_DB[0], min(GAIN_DB[1], TARGET_DB - rms_db)) / 20)


def for_speaker(samples: list[int], rate: int, **kw) -> list[int]:
    t = {**TUNING, **kw}
    g = level(samples, rate) if t.get("level", 0) else 1.0  # off unless asked (the "enhance" option)
    x = [float(v) * g for v in samples]
    vb = virtual_bass(x, rate, t["cut_hz"], t["vbass"]) if t["vbass"] else None
    x = highpass(x, rate, t["cut_hz"])
    if t.get("cut_order", 4) == 4:
        x = highpass(x, rate, t["cut_hz"])  # 4th order: what would only shake the driver
    if vb:
        x = [a + b for a, b in zip(x, vb)]
    if t.get("body_db"):
        x = peak(x, rate, t["body_hz"], t["body_db"], q=0.9)
    x = peak(x, rate, t["reso_hz"], t["reso_db"], q=t["reso_q"])
    x = high_shelf(x, rate, t.get("top_hz", 3000.0), t["top_db"])
    if t.get("tame", 1):
        x = tame(x, rate)
    # volume smoothing (the compressor's gain riding); off: only the make-up to the ceiling and its limiter
    x = compress(x, rate, ratio=2.5 if t.get("smooth", 1) else 1.0, ceiling=t["ceiling"])
    # the band under split_hz has its own, lower limit (the driver runs out of travel there first)
    lo = lowpass(lowpass(x, rate, t["split_hz"]), rate, t["split_hz"])
    lim = t["low_ceiling"] * 32767
    x = [v - l + lim * math.tanh(l / lim) for v, l in zip(x, lo)]  # soft: a hard clip would itself buzz
    lim = t["ceiling"] * 32767
    return [int(max(-lim, min(lim, v))) for v in x]
