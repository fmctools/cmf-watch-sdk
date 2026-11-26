import argparse
import asyncio
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import zlib

from bleak.exc import BleakError

from . import blegatt, channel, firmware, h4, icon, install, link, package, paths


def cmd_pack(args):
    image = args.image.read_bytes()
    elf = args.elf.read_bytes()
    entry = package.check_elf(image, elf)
    kind = package.KIND_LOADER if args.kind == "loader" else package.KIND_APP
    icons = package.widget_icons_dir(args.widget_icons) if args.widget_icons else ()
    if args.widget_icons and not icons:
        raise ValueError(f"no PNG in {args.widget_icons}")
    data = package.pack(image, name=args.name, version=args.version, kind=kind, entry=entry,
                        relocs=package.relocations(elf), abi_minor=package.abi_minor(image, elf), widget_icons=icons,
                        model=args.model)
    with args.output.open("xb") as out:
        out.write(data)


def cmd_info(args):
    pkg = package.parse(args.package.read_bytes())
    print(json.dumps({
        "name": pkg.name, "version": pkg.version, "kind": pkg.kind, "abi": pkg.abi, "abi_minor": pkg.abi_minor,
        "relocs": len(pkg.relocs), "entry": pkg.entry,
        "image_size": len(pkg.image), "crc32": f"{zlib.crc32(pkg.image):#010x}",
        "icon": icon.parse(pkg.image) is not None, "features": package.features(pkg.image),
        "widget_icons": len(pkg.widget_icons), "model": pkg.model,
    }, indent=2))


def cmd_order(args):
    data = package.set_order(args.loader.read_bytes(), args.names)
    with args.output.open("xb") as out:
        out.write(data)


def cmd_firmware(args):
    if args.motor_only and args.stub:
        raise ValueError("--motor-only takes no --stub")
    stub = args.stub.read_bytes() if args.stub else None
    if args.model == "wp3":
        if not stub or args.motor_only or args.motor != 1000:
            raise ValueError("--model wp3 takes only --stub (target/wp3-stub.bin)")
        with args.output.open("xb") as out:
            out.write(firmware.patch_wp3(paths.stock_app_wp3(), stub, label=not args.no_fmc_label))
        return
    motor = None if args.motor == 1000 else args.motor
    data = firmware.patch(paths.stock_app(), stub, motor=motor, reserve=not args.motor_only, weak=None if args.motor_only else args.weak or None,
                          label=not (args.no_fmc_label or args.motor_only))
    with args.output.open("xb") as out:
        out.write(data)


def cmd_ota_image(args):
    wp3 = args.model == "wp3"
    name = paths.FIRMWARE_WP3 if wp3 else paths.FIRMWARE
    stock, app = paths.stock_app_wp3() if wp3 else paths.stock_app(), args.app.read_bytes()
    firmware.check_acth(app)
    if app == stock:
        raise ValueError("the app.bin is stock")
    if len(app) < len(stock):
        raise ValueError("the app.bin is shorter than stock")
    # the header does not name the board: the code at the stock's reset vector must be this model's
    if app[0x200:0x400] != stock[0x200:0x400]:
        raise ValueError(f"the app.bin is not built from {name.name}; check --model")
    with tempfile.TemporaryDirectory() as tmp:
        built = ota_image(paths.firmware(name), app, Path(tmp), args.version_code, temp_only=args.temp_only)
        if paths.app_from(built) != app:
            raise ValueError("the rebuilt image does not carry this app.bin")
        print(paths.fwkit("verify", built).strip())
        with args.output.open("xb") as out, built.open("rb") as src:
            shutil.copyfileobj(src, out)
    if len(app) == len(stock):
        for a, b in firmware.changed_ranges(stock, app):
            print(f"app.bin {firmware.APP_VBASE + a:#x}..{firmware.APP_VBASE + b:#x}: {b - a} byte(s)")
    else:
        print(f"app.bin {len(stock):#x} -> {len(app):#x} bytes")


def _edit_xml(path: Path, file_name: str | None = None, **fields: int) -> None:
    path.write_bytes(firmware.ota_xml_set(path.read_bytes().decode(), file_name, **fields).encode())


def ota_image(image: Path, app: bytes, tmp: Path, version_code: int | None = None, temp_only: bool = False) -> Path:
    """`image` with app.bin inside TEMP.bin (outer AOTA -> TEMP.bin -> app.bin) replaced by `app`."""
    outer, inner, temp, out = tmp / "outer", tmp / "inner", tmp / "TEMP.bin", tmp / "out.bin"
    paths.fwkit("unpack", image, outer)
    paths.fwkit("unpack", "--no-raw", outer / "raw/TEMP.bin.bin", inner)
    (inner / "parts/app.bin.part").write_bytes(app)
    _edit_xml(inner / "ota.xml", "app.bin", file_size=len(app), orig_size=len(app), checksum=zlib.crc32(app))
    paths.fwkit("pack", inner, temp)
    stored = firmware.xz_chunks(firmware.fix_aota_crc(temp.read_bytes()))
    (outer / "parts/TEMP.bin.part").write_bytes(stored)
    # stock: TEMP.bin's file_size, orig_size and checksum all describe the stored (compressed) bytes
    _edit_xml(outer / "ota.xml", "TEMP.bin", file_size=len(stored), orig_size=len(stored), checksum=zlib.crc32(stored))
    if version_code is not None:
        _edit_xml(outer / "ota.xml", version_code=version_code)
    if temp_only:  # what `cmf-watch-utils emu ota` sends: entries ota.xml + TEMP.bin, no AGPS trailer
        hdr = bytearray((outer / "hdr.bin").read_bytes())
        if hdr[0x220:0x228] != b"TEMP.bin":
            raise ValueError("TEMP.bin is not the first AOTA entry after ota.xml")
        struct.pack_into("<I", hdr, 0x0C, 2)
        (outer / "hdr.bin").write_bytes(hdr)
        xml = outer / "ota.xml"
        xml.write_bytes(firmware.ota_xml_only(xml.read_bytes().decode(), "TEMP.bin").encode())
        (outer / "agps.bin").unlink()
    paths.fwkit("pack", outer, out)
    out.write_bytes(firmware.fix_aota_crc(out.read_bytes()))
    return out


def cmd_nor(args):
    data = firmware.stage_nor(args.nor.read_bytes(), args.app.read_bytes())
    with args.output.open("xb") as out:
        out.write(data)


def slot_arg(text):
    if text.upper() == "L":
        return install.LOADER_SLOT
    if not text.isdigit() or int(text) >= install.SLOTS:
        raise argparse.ArgumentTypeError("slot must be 0..15 (8..15 from stub 8) or L")
    return int(text)


def app_slot_arg(text):
    if not text.isdigit() or int(text) >= install.SLOTS:
        raise argparse.ArgumentTypeError("slot must be 0..15 (8..15 from stub 8)")
    return int(text)


def slot_name(slot):
    return "L" if slot == install.LOADER_SLOT else str(slot)


SESSIONS = Path(os.environ.get("WP2APPS_SESSIONS") or Path.home() / ".cache/wp2apps/sessions.json")


def cached_key(target: str) -> bytes | None:
    try:
        return bytes.fromhex(json.loads(SESSIONS.read_text())[target])
    except (OSError, ValueError, KeyError):
        return None


def cache_key(target: str, key: bytes) -> None:
    # the session key is personal data (AGENTS.md): user-only file outside the repository
    try:
        known = json.loads(SESSIONS.read_text())
    except (OSError, ValueError):
        known = {}
    known[target] = key.hex()
    SESSIONS.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(SESSIONS, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as out:
        json.dump(known, out)


async def with_watch(args, action, resume: bool = False):
    """resume: reuse the cached session key of this watch (send, listen), so a running app sees no bind popup."""
    transport = blegatt.gatt_over_ble(args.address) if args.address else h4.gatt_over_h4(args.hci_port)
    target = f"ble:{args.address}" if args.address else f"hci:{args.hci_port}"
    async with transport as gatt:
        session = link.Session()
        await session.handshake(gatt, key=cached_key(target) if resume else None)
        if resume:
            cache_key(target, session.key)
        return await action(session)


def cmd_install(args):
    data = args.package.read_bytes()
    slot = asyncio.run(with_watch(args, lambda s: install.install(s, data, args.slot)))
    print(json.dumps({"installed": slot_name(slot)}))


def cmd_push(args):
    if args.blob is not None:
        kind, data = install.PUSH_BLOB, bytes.fromhex(args.blob)
    elif args.title is not None:
        kind, data = install.PUSH_WIDGET, install.widget_record(args.title, args.value)
    else:
        kind, data = install.PUSH_WIDGET, b""
    asyncio.run(with_watch(args, lambda s: install.push(s, args.slot, kind, data)))
    print(json.dumps({"pushed": slot_name(args.slot), "kind": ("widget", "blob")[kind], "bytes": len(data)}))


def column_arg(text: str):
    """caption:icon:value, split from the right (a caption may be 23:00), icon an index or empty for none"""
    parts = text.rsplit(":", 2)
    if len(parts) != 3 or not (parts[1] == "" or parts[1].isdigit()):
        raise argparse.ArgumentTypeError(f"{text!r} is not caption:icon:value")
    return parts[0], int(parts[1]) if parts[1] else None, parts[2]


def cmd_widget(args):
    data = install.columns_record(args.columns)

    async def run(s):
        if (await install.stub(s)).version < install.STUB_V2:
            raise ValueError(f"columns cards need stub {install.STUB_V2}")
        await install.push(s, args.slot, install.PUSH_WIDGET, data)
    asyncio.run(with_watch(args, run))
    print(json.dumps({"pushed": slot_name(args.slot), "template": "columns", "bytes": len(data)}))


def cmd_remove(args):
    asyncio.run(with_watch(args, lambda s: install.remove(s, args.slot)))
    print(json.dumps({"removed": slot_name(args.slot)}))


def cmd_list(args):
    async def both(s):
        stub = await install.stub(s)
        return stub, await install.listing(s, install.slots(stub))

    stub, found = asyncio.run(with_watch(args, both))
    print(json.dumps({"stub": {"version": stub.version, "abi_minor": stub.abi_minor,
                               "model": install.MODEL_NAMES[stub.model]}, "slots": {slot_name(n): {"name": h.name, "version": h.version, "kind": h.kind,
                                               "abi_minor": h.abi_minor, "icon": h.icon}
                                for n, h in found.items()}}))


def cmd_send(args):
    data = bytes.fromhex(args.data) if args.hex else args.data.encode()
    asyncio.run(with_watch(args, lambda s: channel.send(s, data, args.slot, args.wait), resume=True))
    print(json.dumps({"sent": len(data)}))


def cmd_listen(args):
    async def listen(s):
        state = await channel.ping(s)
        print(json.dumps({"running": slot_name(state.running_slot) if state.running_slot != 0xFF else None,
                          "abi_1_1": bool(state.flags & 1), "paused": bool(state.flags & 2)}), flush=True)
        loop = asyncio.get_running_loop()
        end = None if args.seconds is None else loop.time() + args.seconds
        count = 0
        while args.count is None or count < args.count:
            left = None if end is None else end - loop.time()
            if left is not None and left <= 0:
                break
            try:
                m = await channel.recv(s, 5.0 if left is None else min(left, 5.0))
            except TimeoutError:
                continue
            if args.slot not in (channel.ANY, m.slot):
                continue
            count += 1
            print(json.dumps({"seq": m.seq, "slot": m.slot, "data": m.data.hex(), "text": channel.text(m.data)}),
                  flush=True)

    asyncio.run(with_watch(args, listen, resume=True))


def cmd_chat(args):
    """Lines typed here go to the running app; what it sends back is printed (api-demo shows both in its log)."""
    async def chat(s):
        loop, lines = asyncio.get_running_loop(), asyncio.Queue()
        loop.add_reader(sys.stdin, lambda: lines.put_nowait(sys.stdin.readline()))
        print("chat: type a line and Enter; Ctrl-C ends", flush=True)
        while True:
            while not lines.empty():
                line = lines.get_nowait()
                if line == "":  # EOF
                    return
                if line.strip():
                    await channel.send(s, line.rstrip("\n").encode(), args.slot, 5.0)
            try:
                m = await channel.recv(s, 0.3)
            except TimeoutError:
                continue
            if args.slot in (channel.ANY, m.slot):
                print(f"< {channel.text(m.data)}", flush=True)

    try:
        asyncio.run(with_watch(args, chat, resume=True))
    except KeyboardInterrupt:
        pass


def cmd_herdr(args):
    from . import herdr
    return herdr.main(args)


def cmd_bridge(args):
    from . import bridge
    if not (args.address or args.hci_port):
        raise ValueError("give --address (watch) or --hci-port (emulator)")
    print(json.dumps({"listening": args.port}), flush=True)
    asyncio.run(bridge.serve(args.port, args.address, args.hci_port))


def cmd_cards(args):
    from . import cards

    if args.action == "set":  # a bad spec fails here, before the watch is touched
        new = cards.Cards(args.wrap, tuple(cards.parse_card(c) for c in args.card))
        cards.encode_set(new)

    async def run(s):
        nonlocal new
        if args.action == "get":
            return await cards.get(s)
        if args.action != "set":
            new = cards.with_apps(await cards.get(s))
        await cards.put(s, new)
        return await cards.get(s)

    print(json.dumps(cards.to_json(asyncio.run(with_watch(args, run)))))


def cmd_scan(args):
    found = asyncio.run(blegatt.scan(args.timeout))
    print(json.dumps({"devices": [
        {"name": name, "rssi": rssi, "address": address if args.show_address else "<redacted>"}
        for name, rssi, address in found]}))


def parser():
    top = argparse.ArgumentParser(prog="wp2apps")
    commands = top.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("pack", help="Wrap a raw app image in a WP2 app package")
    pack.add_argument("image", type=Path)
    pack.add_argument("--name", required=True)
    pack.add_argument("--version", type=int, required=True)
    pack.add_argument("--output", type=Path, required=True)
    pack.add_argument("--elf", type=Path, required=True, help="Linked ELF: entry/length check and relocation table")
    pack.add_argument("--kind", choices=("app", "loader"), default="app")
    pack.add_argument("--model", choices=tuple(package.LOADER_MODELS), default="wp2",
                      help="the watch a loader is built for (header byte 0x38); apps run on every model")
    pack.add_argument("--widget-icons", type=Path, metavar="DIR",
                      help="40x40 RGBA PNGs, file name order = icon index (docs/app-icons.md)")
    pack.set_defaults(handler=cmd_pack)
    info = commands.add_parser("info", help="Validate a package and print its header")
    info.add_argument("package", type=Path)
    info.set_defaults(handler=cmd_info)
    order = commands.add_parser("order", help="Loader package with this Apps menu order (docs/app-order.md); "
                                "install it like any loader")
    order.add_argument("loader", type=Path)
    order.add_argument("names", nargs="*", help="package names, first row first; empty: slot order")
    order.add_argument("--output", type=Path, required=True)
    order.set_defaults(handler=cmd_order)
    fw = commands.add_parser("firmware", help="Patch the stock app.bin (heap reservation, stub, motor strength, weak vibration level)")
    fw.add_argument("--stub", type=Path)
    fw.add_argument("--model", choices=("wp2", "wp3"), default="wp2",
                    help="wp3: Watch 3 Pro 1.0.1.77, only the boot watch face guard stub (fw/wp3-stub)")
    fw.add_argument("--motor", type=int, default=1000, metavar="PERMILLE",
                    help="stock vibrations at this share of their strength (default 1000: stock)")
    fw.add_argument("--weak", type=int, default=350, metavar="PERMILLE",
                    help="a fourth vibration level in Settings at this share (default 350; 0: none)")
    fw.add_argument("--motor-only", action="store_true",
                    help="only the motor patch: no heap reservation, no stub, no label, stock app.bin size")
    fw.add_argument("--no-fmc-label", action="store_true",
                    help="keep the stock About page; by default Settings > About shows '<version>-fmc' "
                         "(AT GETVERSION and every other version stay stock)")
    fw.add_argument("--output", type=Path, required=True)
    fw.set_defaults(handler=cmd_firmware)
    ota = commands.add_parser("ota-image", help="Full stock firmware image carrying a patched app.bin")
    ota.add_argument("--app", type=Path, required=True)
    ota.add_argument("--model", choices=("wp2", "wp3"), default="wp2", help="wp3: Watch 3 Pro 1.0.1.77")
    ota.add_argument("--output", type=Path, required=True)
    ota.add_argument("--version-code", type=lambda v: int(v, 0), metavar="N",
                     help="ota.xml version_code (stock 0x10000)")
    ota.add_argument("--temp-only", action="store_true",
                     help="only TEMP.bin (app + sdfs), no resources or AGPS trailer: the emulator's OTA transport")
    ota.set_defaults(handler=cmd_ota_image)
    nor = commands.add_parser("nor", help="Put a patched app.bin into a NOR image")
    nor.add_argument("nor", type=Path)
    nor.add_argument("--app", type=Path, required=True)
    nor.add_argument("--output", type=Path, required=True)
    nor.set_defaults(handler=cmd_nor)
    for name, handler, help_ in (("install", cmd_install, "Install a package over BLE"),
                                 ("remove", cmd_remove, "Delete a slot and its app data"),
                                 ("list", cmd_list, "List installed packages"),
                                 ("send", cmd_send, "Send one message to the running app (ABI 1.1)"),
                                 ("listen", cmd_listen, "Print the running app's messages as JSON lines"),
                                 ("chat", cmd_chat, "Type lines to the running app, print what it sends back"),
                                 ("push", cmd_push, "Set an app's widget text or start blob while it does not run "
                                  "(stub 8, docs/widget-push.md); no --title/--blob clears the widget")):
        sub = commands.add_parser(name, help=help_)
        if name == "install":
            sub.add_argument("package", type=Path)
            sub.add_argument("--slot", type=slot_arg)
        if name == "remove":
            sub.add_argument("--slot", type=slot_arg, required=True)
        if name == "push":
            sub.add_argument("--slot", type=app_slot_arg, required=True)
            what = sub.add_mutually_exclusive_group()
            what.add_argument("--title", help="widget title (<= 23 bytes); --value is the text below it")
            what.add_argument("--blob", metavar="HEX", help="start blob (<= 128 bytes); empty string removes it")
            sub.add_argument("--value", default="", help="widget value (<= 39 bytes, \\n for a second line)")
        if name in ("send", "listen", "chat"):
            sub.add_argument("--slot", type=app_slot_arg, default=channel.ANY,
                             help="only this app slot (default: whatever runs)")
        if name == "send":
            sub.add_argument("data")
            sub.add_argument("--hex", action="store_true", help="DATA is hex, not UTF-8 text")
            sub.add_argument("--wait", type=float, default=10.0, help="seconds to retry a full or paused inbox")
        if name == "listen":
            sub.add_argument("--count", type=int)
            sub.add_argument("--seconds", type=float)
        where = sub.add_mutually_exclusive_group(required=True)
        where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
        where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
        sub.set_defaults(handler=handler)
    herdr = commands.add_parser("herdr", help="Daemon for the herdr-agents app (docs/herdr-agents.md)")
    where = herdr.add_mutually_exclusive_group(required=True)
    where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
    where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
    where.add_argument("--sim", type=int, metavar="PORT", help="app in the host simulator (scripts/sim --tcp PORT)")
    herdr.add_argument("--herdr", default="herdr", help="herdr executable")
    herdr.add_argument("--period", type=float, default=1.0, help="seconds between `herdr agent list` polls")
    herdr.add_argument("-v", "--verbose", action="store_true", help="log what is sent, titles included")
    herdr.add_argument("--chat", action="store_true",
                       help="also a chat: typed lines go to the running app, its other messages are printed")
    herdr.set_defaults(handler=cmd_herdr)
    br = commands.add_parser("bridge", help="TCP hex-line bridge to the running app over the K channel "
                                           "(docs/abi-1.4.md)")
    br.add_argument("--port", type=int, default=7700)
    where = br.add_mutually_exclusive_group()
    where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
    where.add_argument("--address", help="BLE address of a real watch")
    br.set_defaults(handler=cmd_bridge)
    cards_cmd = commands.add_parser("cards", help="The cards left of the watch face (stock 0x906D, docs/recon.md "
                                                  "\"Widget cards\"); set and add-apps write to the watch")
    actions = cards_cmd.add_subparsers(dest="action", required=True)
    for name, help_ in (("get", "Print the card list"),
                        ("set", "Replace the list: view:ID (a stock view) or widgets:ID[/PARAM],... (up to 4)"),
                        ("add-apps", "Append a card with the stub's Apps widget (id 0x40) unless one has it")):
        sub = actions.add_parser(name, help=help_)
        if name == "set":
            sub.add_argument("card", nargs="*", help="up to 8 cards, in page order")
            sub.add_argument("--wrap", action="store_true", help="the pages wrap around")
        where = sub.add_mutually_exclusive_group(required=True)
        where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
        where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
        sub.set_defaults(handler=cmd_cards)
    widget = commands.add_parser("widget", help="An app's widget card (stub 10 templates, docs/widget-push.md)")
    actions = widget.add_subparsers(dest="action", required=True)
    sub = actions.add_parser("set", help="PUSH a columns card while the app does not run")
    sub.add_argument("slot", type=app_slot_arg)
    sub.add_argument("--columns", type=column_arg, nargs="+", required=True, metavar="CAPTION:ICON:VALUE",
                     help="1..4 columns, e.g. 'Now:0:-3°' '23:00:1:-4°'; an empty ICON shows none")
    where = sub.add_mutually_exclusive_group(required=True)
    where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
    where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
    sub.set_defaults(handler=cmd_widget)
    scan = commands.add_parser("scan", help="List nearby CMF watches (read-only; address redacted by default)")
    scan.add_argument("--timeout", type=float, default=5.0)
    scan.add_argument("--show-address", action="store_true")
    scan.set_defaults(handler=cmd_scan)
    return top


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        return args.handler(args) or 0
    except channel.NoApp as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    except (OSError, ValueError, RuntimeError, ConnectionError, BleakError, struct.error) as error:
        print(f"Error: {error or type(error).__name__}", file=sys.stderr)
        return 1
    except KeyError as error:
        print(f"Error: missing {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
