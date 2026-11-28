"""IMA ADPCM (IMA/DVI standard, 4 bits a sample), the phone side of the watch's speaker stream.

Blocks carry their own start state (predictor i16 LE, index u8) and then codes, low nibble first, so the watch
decodes every message on its own (sdk/cmf-wp2-app/src/adpcm.rs, same vectors).
"""
import struct

STEPS = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97,
    107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871,
    5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623,
    27086, 29794, 32767,
]
INDEX = [-1, -1, -1, -1, 2, 4, 6, 8]
HEADER = 3


def _step(predictor: int, index: int, code: int) -> tuple[int, int]:
    step = STEPS[index]
    diff = step >> 3
    if code & 4:
        diff += step
    if code & 2:
        diff += step >> 1
    if code & 1:
        diff += step >> 2
    predictor += -diff if code & 8 else diff
    return max(-32768, min(32767, predictor)), max(0, min(88, index + INDEX[code & 7]))


class Encoder:
    """Keeps the state across blocks, so consecutive blocks join without a click.

    `paths` > 0: a trellis search over each block instead of one code at a time. At each sample every kept path
    tries the standard code and its magnitude ±1 (and the other sign when the standard one is 0); paths reaching
    the same (predictor, index) keep the lower squared error; the `paths` best go on. ~1-1.5 dB less noise than
    the standard encoder on music (4 paths: +1 dB), decoded by the same standard decoder. CMFX (HubKit Speaker.swift)
    does the same, byte for byte: the order of the candidates and of the kept paths is part of it.
    """

    def __init__(self, paths: int = 0):
        self.predictor, self.index = 0, 0
        self.paths = paths

    def _code(self, sample: int) -> int:
        step = STEPS[self.index]
        diff = sample - self.predictor
        code = 8 if diff < 0 else 0
        diff = abs(diff)
        if diff >= step:
            code |= 4
            diff -= step
        if diff >= step >> 1:
            code |= 2
            diff -= step >> 1
        if diff >= step >> 2:
            code |= 1
        self.predictor, self.index = _step(self.predictor, self.index, code)
        return code

    def _codes(self, samples) -> list[int]:
        if not self.paths:
            return [self._code(x) for x in samples]
        # a path: (error, predictor, index, codes as a linked list (code, rest))
        paths = [(0, self.predictor, self.index, None)]
        for x in samples:
            nxt: dict[tuple[int, int], tuple] = {}
            for e, p, i, node in paths:
                step = STEPS[i]
                d = x - p
                sign = 8 if d < 0 else 0
                d = abs(d)
                m = 0
                if d >= step:
                    m |= 4
                    d -= step
                if d >= step >> 1:
                    m |= 2
                    d -= step >> 1
                if d >= step >> 2:
                    m |= 1
                cands = [sign | k for k in (m - 1, m, m + 1) if 0 <= k <= 7]
                if m == 0:
                    cands.append(8 - sign)
                for c in cands:
                    p1, i1 = _step(p, i, c)
                    e1 = e + (x - p1) * (x - p1)
                    old = nxt.get((p1, i1))
                    if old is None or e1 < old[0]:
                        nxt[(p1, i1)] = (e1, p1, i1, (c, node))
            # stable: equal errors keep the order the states were first reached in
            paths = sorted(nxt.values(), key=lambda t: t[0])[: self.paths]
        _, self.predictor, self.index, node = paths[0]
        codes = []
        while node:
            codes.append(node[0])
            node = node[1]
        return codes[::-1]

    def block(self, samples) -> bytes:
        """An even number of i16 samples → one block."""
        if len(samples) % 2:
            raise ValueError("a block holds an even number of samples")
        out = bytearray(struct.pack("<hB", self.predictor, self.index))
        codes = self._codes(samples)
        for i in range(0, len(codes), 2):
            out.append(codes[i] | codes[i + 1] << 4)
        return bytes(out)


def decode(block: bytes) -> list[int]:
    predictor, index = struct.unpack_from("<hB", block)
    out = []
    for b in block[HEADER:]:
        for code in (b & 0xF, b >> 4):
            predictor, index = _step(predictor, index, code)
            out.append(predictor)
    return out
