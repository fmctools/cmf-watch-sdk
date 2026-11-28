"""A stand-in phone for voice typing (cmf_wp2_ui::dictate) in any app in the simulator: keeps what the watch
sends as a WAV, answers the words so far every 25 blocks and, at the end, fixed final words. CMFX does the real
thing (ios/App/Hub/Dictation.swift, iOS speech recognition).

  scripts/sim <app> --tcp 7798 &  uv run python hosts/voicehost.py --sim 7798 --words "coffee" out.wav
"""
import argparse, asyncio, pathlib, sys, wave

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps import adpcm  # noqa: E402
from wp2apps.channel import SimChannel  # noqa: E402


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int, required=True)
    ap.add_argument("--words", default="Hello from the simulator")
    ap.add_argument("out", nargs="?", default="voice.wav")
    a = ap.parse_args()
    blocks = {}
    async with SimChannel(a.sim) as ch:
        while True:
            m = (await ch.recv()).data
            if m[:1] != b"~":
                continue
            k = m[1:2]
            if k in (b"1", b"3"):
                blocks = {}
                print("start", "record" if k == b"3" else "dictate", flush=True)
            elif k == b"a":
                blocks[int.from_bytes(m[2:4], "little")] = m[4:]
                if len(blocks) % 25 == 0:
                    await ch.send(b"~\x00" + f"heard {len(blocks)} blocks".encode())
            elif k == b"2":
                print("cancel", flush=True)
            elif k == b"0":
                pcm = [v for i in sorted(blocks) for v in adpcm.decode(blocks[i])]
                with wave.open(a.out, "wb") as w:
                    w.setnchannels(1), w.setsampwidth(2), w.setframerate(8000)
                    w.writeframes(b"".join(int(v).to_bytes(2, "little", signed=True) for v in pcm))
                print(f"end: {len(blocks)} blocks, {len(pcm) / 8000:.2f} s", flush=True)
                await ch.send(b"~\x01" + a.words.encode())


if __name__ == "__main__":
    asyncio.run(main())
