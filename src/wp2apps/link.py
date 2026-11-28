import asyncio
import hashlib
import os
import struct
import zlib

from Crypto.Cipher import AES
from Crypto.Util.Padding import pad, unpad

IV = bytes.fromhex("5051525354555657606162636465665A")
HEAD = 0xF5
HEADER = struct.Struct(">BHHHHH")
SHELL_W = "77d4ff01-2fe2-2334-0d35-9ccd078f529c"
SHELL_N = "77d4ff02-2fe2-2334-0d35-9ccd078f529c"
CMD_W = "0000fff2-0000-1000-8000-00805f9b34fb"
DATA_N = "02f00000-0000-0000-0000-00000000ffe2"
CMD_N = "0000fff1-0000-1000-8000-00805f9b34fb"
SYSTEM = 0xFFFF
# watch→phone frames in the clear declare a length with the CRC but omit it
PLAIN_REPLIES = frozenset({0x0048})
PHONE = b"\xa5CMF Watch Pro 2"
ZERO_KEY = bytes(16)


class Rebind(Exception):
    """Confirming the stock bind popup clears the authkey (0x100CD2CC) and the watch answers with an all-zero key;
    the stub refuses sessions made from it, so the handshake starts over (docs/recon.md)."""


def encrypt(key: bytes, data: bytes) -> bytes:
    body = data + struct.pack("<I", zlib.crc32(data))
    return AES.new(key, AES.MODE_CBC, IV).encrypt(pad(body, 16))


def decrypt(key: bytes, data: bytes) -> bytes:
    if not data or len(data) % 16:
        raise ValueError("ciphertext is not a whole number of blocks")
    try:
        body = unpad(AES.new(key, AES.MODE_CBC, IV).decrypt(data), 16)
    except ValueError as error:
        raise ValueError("bad padding") from error
    if len(body) < 4 or zlib.crc32(body[:-4]) != struct.unpack("<I", body[-4:])[0]:
        raise ValueError("payload CRC mismatch")
    return body[:-4]


STUB_INFO = 0x90A0  # install.INFO, answered by the stub only with a bound session
STUB_EVENT = 0xB0A6  # app -> host frames, unsolicited (docs/abi-1.1.md)
PING_REPLY = 0x90A7 | 0xA000  # the stub's answer to a PING (channel.ping)


def frame(c1: int, c2: int, payload: bytes, key: bytes | None) -> bytes:
    body = encrypt(key, payload) if key else payload + struct.pack("<I", zlib.crc32(payload))
    return HEADER.pack(HEAD, len(body), c1, 1, 1, c2) + body


class Parser:
    def __init__(self):
        self.buf = b""

    def feed(self, data: bytes) -> list[tuple[int, int, bytes]]:
        self.buf += data
        out = []
        while True:
            start = self.buf.find(bytes([HEAD]))
            if start < 0:
                self.buf = b""
                break
            self.buf = self.buf[start:]
            if len(self.buf) < HEADER.size:
                break
            _, length, c1, _, _, c2 = HEADER.unpack_from(self.buf)
            size = length - 4 if c1 == SYSTEM and c2 in PLAIN_REPLIES else length
            if len(self.buf) < HEADER.size + size:
                break
            out.append((c1, c2, self.buf[HEADER.size:HEADER.size + size]))
            self.buf = self.buf[HEADER.size + size:]
        return out


def authkey(rnd1: bytes, rnd2: bytes, secret: bytes) -> bytes:
    return hashlib.sha256(rnd1 + rnd2 + secret).digest()[:16]


def channel_key(secret_hex: str) -> bytes:
    """Key K of the stub's own channel (docs/abi-1.4.md): the pairing secret itself (NVRAM `APP_SECRET`, 32 hex
    chars), so the stub needs no SHA-256. No bind, no popup, survives reboots."""
    return bytes.fromhex(secret_hex)


def session_key(nonce: bytes, auth: bytes) -> bytes:
    return hashlib.sha256(nonce[:16] + auth).digest()[:16]


class Session:
    def __init__(self):
        self.gatt = None
        self.key = None
        self.frames = asyncio.Queue()
        # PING replies on their own: a keep-alive PING and a stream of messages in flight (channel.Pipe) don't take
        # each other's replies
        self.pings = asyncio.Queue()
        self.events = asyncio.Queue(maxsize=256)
        self.shell = asyncio.Queue()
        self.parser = Parser()

    async def attach(self, gatt):
        self.gatt = gatt
        await self.gatt.subscribe(CMD_N, lambda data: [self._route(f) for f in self.parser.feed(data)])
        await self.gatt.subscribe(SHELL_N, self.shell.put_nowait)
        # the stock app treats the third subscription as "phone ready"; the shell is silent before it
        await self.gatt.subscribe(DATA_N, lambda data: None)
        await asyncio.sleep(0.6)

    def _route(self, f) -> None:
        if f[:2] == (SYSTEM, PING_REPLY):
            self.pings.put_nowait(f)
        elif f[:2] != (SYSTEM, STUB_EVENT):
            self.frames.put_nowait(f)
        elif not self.events.full():  # a host that never reads them drops the oldest ones' successors
            self.events.put_nowait(f[2])

    async def at(self, command: str, timeout: float = 6.0) -> bytes:
        await self.gatt.write(SHELL_W, command.encode() + b"\r\n")
        reply = b""
        async with asyncio.timeout(timeout):
            while not any(end in reply for end in (b"OK", b"ERROR", b"FAIL")):
                reply += await self.shell.get()
        return reply

    async def _shell_value(self, field: str) -> str:
        text = (await self.at(f"AT {field}")).decode(errors="replace").strip()
        if not text.startswith(f"{field}:") or not text.endswith(",OK"):
            raise ValueError(f"unexpected {field} reply {text!r}")
        return text[len(field) + 1:-len(",OK")]

    async def _exchange(self, c2, payload, expect, key, timeout):
        await self.gatt.write(CMD_W, frame(SYSTEM, c2, payload, key), response=False)
        async with asyncio.timeout(timeout):
            while True:
                c1, got, body = await self.frames.get()
                if c1 == SYSTEM and got == expect:
                    return body

    async def request(self, c1: int, c2: int, payload: bytes, expect: int, timeout: float = 6.0,
                      key: bytes | None = None) -> bytes:
        if c1 != SYSTEM:
            raise ValueError("only system commands are supported")
        key = key or self.key
        await self.gatt.write(CMD_W, frame(SYSTEM, c2, payload, key), response=False)
        # on a shared link (iOS) the notifications carry other hosts' frames too: skip what this key cannot open
        q = self.pings if expect == PING_REPLY else self.frames
        async with asyncio.timeout(timeout):
            while True:
                got_c1, got, body = await q.get()
                if got_c1 == SYSTEM and got == expect:
                    try:
                        return decrypt(key, body)
                    except ValueError:
                        continue

    async def _auth_request(self, c2: int, payload: bytes, expect: int, auth: bytes, timeout: float = 6.0) -> bytes:
        body = await self._exchange(c2, payload, expect, auth, timeout)
        try:
            return decrypt(auth, body)
        except ValueError:
            decrypt(ZERO_KEY, body)  # raises unless the watch cleared its authkey
            raise Rebind from None

    async def _bind(self, secret: bytes) -> tuple[bytes, bytes]:
        while not self.frames.empty():
            self.frames.get_nowait()
        rnd1 = os.urandom(16)
        reply = await self._exchange(0x8047, rnd1 + hashlib.sha256(rnd1 + secret).digest(), 0x0048, None, 6.0)
        rnd2 = reply[:16]
        if reply[16:48] != hashlib.sha256(rnd2 + secret).digest():
            raise ValueError("watch failed the pairing challenge")
        auth = authkey(rnd1, rnd2, secret)
        # after 0x8047 the first 0x8049 only opens the stock bind popup; the next one is answered at once
        await self.gatt.write(CMD_W, frame(SYSTEM, 0x8049, PHONE, auth), response=False)
        try:
            await self._auth_request(0x8049, PHONE, 0x0049, auth, timeout=2.0)
        except TimeoutError:
            await self._auth_request(0x8049, PHONE, 0x0049, auth)
        return auth, await self._auth_request(0x804B, b"\xa5", 0x004C, auth)

    async def resume(self, key: bytes) -> bool:
        """Reuses a session key of this boot: no bind, so no stock "Paired" popup (closing it jumps to the clock and
        ends a running app). False if the stub does not answer with it."""
        self.key = key
        try:
            await self.request(SYSTEM, STUB_INFO, b"\xff", STUB_INFO | 0xA000, timeout=2.0)
            return True
        except (TimeoutError, ValueError):
            self.key = None
            return False

    async def open_channel(self, gatt) -> None:
        """K session: reads the pairing secret and keys app frames with K; the stock session is left alone."""
        await self.attach(gatt)
        for attempt in range(3):  # the shell sometimes stays silent right after a reconnect (emulator)
            try:
                self.key = channel_key(await self._shell_value("GETSECRET"))
                return
            except TimeoutError:
                if attempt == 2:
                    raise

    async def handshake(self, gatt, attempts: int = 3, key: bytes | None = None) -> None:
        await self.attach(gatt)
        if key and await self.resume(key):
            return
        secret = bytes.fromhex(await self._shell_value("GETSECRET"))
        for _ in range(attempts):
            try:
                auth, nonce = await self._bind(secret)
            except Rebind:
                continue
            self.key = session_key(nonce, auth)
            # no 0x8004 time sync: a clock off by over 2 min makes the stock app "recover" (factory reset)
            await self.request(SYSTEM, 0x804D, b"\xa5", 0x0004)
            # a tap on the bind popup after 0x8049 zeroes the authkey and the stub goes silent: check, else bind again
            try:
                await self.request(SYSTEM, STUB_INFO, b"\xff", STUB_INFO | 0xA000, timeout=2.0)
                return
            except TimeoutError:
                continue
        raise ValueError("no session: the bind popup on the watch was confirmed during every handshake (try again "
                         "without touching it), or the watch firmware has no wp2apps stub")
