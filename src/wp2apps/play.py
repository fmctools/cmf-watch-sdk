"""`wp2apps play FILE`: streams an audio file to the Speaker app (sdk/examples/speaker) as IMA ADPCM, 8 kHz mono.

A real watch over macOS BLE took ~4 messages per ~90 ms with stop-and-wait batches (0.68x of what 16 kHz needs,
recorded 2026-11-21): the stream uses Pipe.stream, a sliding window, instead.

macOS `afconvert` reads the file (MP3, M4A/AAC, WAV, AIFF, FLAC: whatever Core Audio plays). The app reports
credit (`F next room`) and the host sends no block it has no room for, so the stream runs at the watch's speed.
"""
import asyncio
import shutil
import subprocess
import sys
import tempfile
import time
import wave
from pathlib import Path

from . import adpcm, channel, tune

OUT_KHZ = 16  # what the watch plays
RATE = 10667  # what goes over BLE: 2/3 of it, upsampled 2 -> 3 by the app (`o 16 1`)
CODES = 122  # code bytes a message: `a` seq:u16, predictor, index, codes = 128 B
SAMPLES = 2 * CODES
GROUP = channel.WINDOW


def pcm16k(path: Path, start: float = 0.0, seconds: float | None = None) -> list[int]:
    """The file as 16 kHz mono i16 samples."""
    if path.suffix.lower() == ".wav":
        with wave.open(str(path)) as w:
            if (w.getframerate(), w.getnchannels(), w.getsampwidth()) == (RATE, 1, 2):
                return _cut(_samples(w.readframes(w.getnframes())), start, seconds)
    if not shutil.which("afconvert"):
        raise SystemExit("afconvert (macOS) is needed to read this file; give a 16 kHz mono 16-bit WAV instead")
    with tempfile.TemporaryDirectory() as d:
        out = Path(d) / "pcm.wav"
        subprocess.run(["afconvert", "-f", "WAVE", "-d", f"LEI16@{RATE}", "-c", "1", str(path), str(out)],
                       check=True, capture_output=True)
        with wave.open(str(out)) as w:
            return _cut(_samples(w.readframes(w.getnframes())), start, seconds)


def _samples(raw: bytes) -> list[int]:
    import array
    a = array.array("h")
    a.frombytes(raw[: len(raw) // 2 * 2])
    if sys.byteorder != "little":
        a.byteswap()
    return a.tolist()


def _cut(s: list[int], start: float, seconds: float | None) -> list[int]:
    a = int(start * RATE)
    return s[a:] if seconds is None else s[a:a + int(seconds * RATE)]


PATHS = 4  # the trellis encoder's paths (adpcm.Encoder) when enhanced: +1 dB over the standard one, ~14x real time


def messages(samples: list[int], enc: "adpcm.Encoder | None" = None, seq0: int = 0) -> list[bytes]:
    """`a seq block` messages, one encoder across them (no click between blocks); `enc` and `seq0` carry on from
    an earlier part of the same song."""
    enc = enc or adpcm.Encoder()
    out = []
    for seq, i in enumerate(range(0, len(samples), SAMPLES)):
        chunk = samples[i:i + SAMPLES]
        chunk += [0] * (SAMPLES - len(chunk))
        out.append(b"a" + ((seq0 + seq) & 0xFFFF).to_bytes(2, "little") + enc.block(chunk))
    return out


async def _credit(session, credit: dict, wait: float) -> None:
    """Takes the app's `F next room` reports; polls with PING so its EVENTs come back (docs/abi-1.1.md)."""
    if not session.events.empty():
        body = session.events.get_nowait()
    else:
        try:
            body = await asyncio.wait_for(session.events.get(), wait)
        except TimeoutError:
            await channel.ping(session)
            return
    m = channel.message(session.key, body)
    if m.data[:1] == b"F" and len(m.data) == 4:
        credit["next"] = int.from_bytes(m.data[1:3], "little")
        credit["room"] = m.data[3]
        credit["most"] = max(credit.get("most", 0), m.data[3])
        credit["seen"] = True


async def play(session, samples: list[int], title: str, volume: int | None = None, log=print,
               shape: bool = True, excite: int = 40) -> dict:
    """Credit flow: block `seq` goes out only once the app said it has room for it (`F next room`), so the
    stream follows the watch's DAC clock, also when the watch (or the emulator) runs slower than real time."""
    pipe = channel.Pipe(session)
    head = [b"o" + bytes([OUT_KHZ, 1]), b"t" + title.encode()[:40]]
    head.append(b"x" + bytes([max(0, min(255, excite))]))
    if volume is not None:
        head.append(b"v" + bytes([max(0, min(100, volume))]))
    while not session.events.empty():
        session.events.get_nowait()
    await pipe.send(head)
    if shape:
        samples = tune.for_speaker(samples, RATE)
    msgs = messages(samples + [0] * (RATE * 2 // 5))  # 0.4 s of silence pushes the last music past PREBUFFER
    block_s = SAMPLES / RATE
    credit = {"next": 0, "room": 8, "seen": False}  # before the first report: what the app surely holds
    t0, stalls = time.monotonic(), [0]

    def limit():
        return credit["next"] + credit["room"]

    async def idle():
        stalls[0] += 1
        await _credit(session, credit, 0.03)

    async def after(g):
        while not session.events.empty():
            await _credit(session, credit, 0.0)
        now = time.monotonic()
        if now - credit.setdefault("logged_at", t0) >= 5:
            sent = g - credit.get("logged_g", 0)
            log(f"  {g * block_s:6.1f} s of {len(msgs) * block_s:.1f} s | link {sent * block_s / (now - credit['logged_at']):.2f}x"
                f" real time | watch room {credit['room']} blocks | {pipe.refused} refused", flush=True)
            credit["logged_at"], credit["logged_g"] = now, g

    await pipe.stream(msgs, limit, idle, after)
    stalls = stalls[0]
    # the app's ring drains (its room comes back to the most it ever reported), then the loader's ~0.5 s plays
    full, end = max(credit.get("most", 0), 1), time.monotonic() + 5
    while credit["room"] < full and time.monotonic() < end:
        await _credit(session, credit, 0.1)
    await asyncio.sleep(0.6)
    await pipe.send([b"c"])
    took = time.monotonic() - t0
    return {"blocks": len(msgs), "audio_s": round(len(msgs) * block_s, 2), "took_s": round(took, 2),
            "speed": round(len(msgs) * block_s / took, 2), "refused": pipe.refused, "credit_waits": stalls,
            "credit_seen": credit["seen"]}
