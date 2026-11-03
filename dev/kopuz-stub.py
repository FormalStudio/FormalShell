#!/usr/bin/env python3
"""kopuzd and Kopuz's MPRIS player, for FormalShell's --kopuz leg.

`daemon` serves the slice of kopuzd's gRPC (`kopuz.v1.Kopuz`,
kopuz/crates/proto/proto/kopuz.proto) the shell calls, on a Unix socket,
over HTTP/2 with prior knowledge written by hand: the rig has no grpcio.
It reads what crates/fs-kopuz sends (one call per connection, a header
block of literal fields with no Huffman) and answers with an indexed
`:status 200`, literal fields and gRPC's 5-byte message prefix. The field
numbers below are the proto's. Every call is appended to --log as one JSON
line; --control is a FIFO taking one command per line:

  favorite on|off      the playing key's favourite flips in the daemon, as
                       another frontend would, then LibraryInvalidated{4}
  dont-recommend on|off  the active source's capability, then
                       LibraryInvalidated{6} (sources re-read)
  row playing|other    the row the daemon reports, then a PlayerState event

`player` owns org.mpris.MediaPlayer2.kopuz on the session bus through Gio,
as Kopuz's GUI does, with the tracks from --tracks (a JSON list of
{title, artist, album, length, url}); Next steps through them.
"""
import argparse
import json
import os
import socket
import struct
import sys
import threading
import time

PREFACE = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"
DATA, HEADERS, SETTINGS, PING, GOAWAY = 0x0, 0x1, 0x4, 0x6, 0x7
END_STREAM, ACK, END_HEADERS = 0x1, 0x1, 0x4

TABLE_FAVORITES = 4
TABLE_SERVERS = 6


# ---------------- protobuf ----------------

def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def f_uint(field, v):
    return varint(field << 3) + varint(v) if v else b""


def f_bytes(field, b):
    return varint(field << 3 | 2) + varint(len(b)) + b


def f_str(field, s):
    return f_bytes(field, s.encode()) if s else b""


def fields(buf):
    """field -> [value], varints as ints and length-delimited as bytes."""
    out, i = {}, 0

    def read_varint():
        nonlocal i
        shift = n = 0
        while True:
            b = buf[i]
            i += 1
            n |= (b & 0x7F) << shift
            shift += 7
            if not b & 0x80:
                return n

    while i < len(buf):
        key = read_varint()
        field, wt = key >> 3, key & 7
        if wt == 0:
            v = read_varint()
        elif wt == 2:
            n = read_varint()
            v, i = buf[i:i + n], i + n
        elif wt == 1:
            v, i = buf[i:i + 8], i + 8
        elif wt == 5:
            v, i = buf[i:i + 4], i + 4
        else:
            raise ValueError(f"wire type {wt}")
        out.setdefault(field, []).append(v)
    return out


def track_info(t):
    # TrackInfo: key 1, title 3, artist 4, album 5, duration_ms 7.
    return (f_str(1, t["key"]) + f_str(3, t["title"]) + f_str(4, t["artist"])
            + f_str(5, t["album"]) + f_uint(7, int(t["length"] * 1000)))


def player_state(rev, row):
    # PlayerState: rev 1, row 14.
    return f_uint(1, rev) + (f_bytes(14, track_info(row)) if row else b"")


def lyrics_message(lines):
    # Lyrics.synced 2; LyricLine start_ms 1, end_ms 2, text 3, chunks 4
    # (LyricChunk start_ms 1, text 2).
    out = b""
    for line in lines:
        body = f_uint(1, line["start"]) + f_uint(2, line["end"]) + f_str(3, line["text"])
        for start, text in line.get("chunks", []):
            body += f_bytes(4, f_uint(1, start) + f_str(2, text))
        out += f_bytes(2, body)
    return out


# ---------------- HTTP/2 ----------------

def frame(kind, flags, stream, payload=b""):
    return struct.pack(">I", len(payload))[1:] + bytes([kind, flags]) + struct.pack(">I", stream) + payload


def hpack_int(first, prefix, v):
    top = (1 << prefix) - 1
    if v < top:
        return bytes([first | v])
    out = bytearray([first | top])
    v -= top
    while v >= 0x80:
        out.append(v & 0x7F | 0x80)
        v >>= 7
    out.append(v)
    return bytes(out)


def literal(name, value):
    # Literal without indexing, literal name, no Huffman.
    return (b"\x00" + hpack_int(0, 7, len(name)) + name.encode()
            + hpack_int(0, 7, len(value)) + value.encode())


def header_fields(block):
    """The literal fields of a header block with no Huffman, which is all
    crates/fs-kopuz sends; anything else is refused."""
    out, i = {}, 0

    def read_int(prefix):
        nonlocal i
        top = (1 << prefix) - 1
        v = block[i] & top
        i += 1
        if v < top:
            return v
        shift = 0
        while True:
            b = block[i]
            i += 1
            v += (b & 0x7F) << shift
            shift += 7
            if not b & 0x80:
                return v

    def read_str():
        nonlocal i
        if block[i] & 0x80:
            raise ValueError("Huffman-coded header")
        n = read_int(7)
        s, i = block[i:i + n].decode(), i + n
        return s

    while i < len(block):
        b = block[i]
        if b & 0x80 or b & 0xE0 == 0x20:
            raise ValueError(f"header representation {b:#x}")
        prefix = 6 if b & 0x40 else 4
        if read_int(prefix) != 0:
            raise ValueError("indexed header name")
        name = read_str()
        out[name] = read_str()
    return out


RESPONSE_HEADERS = b"\x88" + literal("content-type", "application/grpc")


class Daemon:
    def __init__(self, args):
        self.log_path = args.log
        self.lock = threading.Lock()
        # The event pusher and a connection's own thread write the same
        # socket; a frame never interleaves with another.
        self.wlock = threading.Lock()
        self.rev = 1
        self.row = "playing"
        self.track = {"key": args.key, "title": args.title, "artist": args.artist,
                      "album": args.album, "length": args.length}
        self.other = dict(self.track, key=args.key + "-other", title=args.title + " (Other Daemon)")
        self.favorites = set()
        self.dont_recommend = False
        self.lyrics = json.load(open(args.lyrics))
        self.subscribers = []

    def log(self, entry):
        with self.lock, open(self.log_path, "a") as f:
            f.write(json.dumps(entry, sort_keys=True) + "\n")

    def current_row(self):
        return self.track if self.row == "playing" else self.other

    def push(self, event):
        # EventEnvelope.event 2.
        envelope = f_bytes(2, event)
        data = frame(DATA, 0, 1, b"\x00" + struct.pack(">I", len(envelope)) + envelope)
        with self.lock:
            live = []
            for conn in self.subscribers:
                try:
                    self.send(conn, data)
                    live.append(conn)
                except OSError:
                    pass
            self.subscribers = live

    def send(self, conn, data):
        with self.wlock:
            conn.sendall(data)

    def invalidate(self, table):
        # Event.library_invalidated 6, LibraryInvalidated.table 1.
        self.push(f_bytes(6, f_uint(1, table)))

    def control(self, path):
        while True:
            with open(path) as f:
                for line in f:
                    words = line.split()
                    if not words:
                        continue
                    self.log({"control": " ".join(words)})
                    what, value = words[0], words[1] if len(words) > 1 else ""
                    if what == "favorite":
                        with self.lock:
                            (self.favorites.add if value == "on" else self.favorites.discard)(self.track["key"])
                        self.invalidate(TABLE_FAVORITES)
                    elif what == "dont-recommend":
                        self.dont_recommend = value == "on"
                        self.invalidate(TABLE_SERVERS)
                    elif what == "row":
                        with self.lock:
                            self.row = value
                            self.rev += 1
                            state = player_state(self.rev, self.current_row())
                        # Event.player_state 1.
                        self.push(f_bytes(1, state))

    def answer(self, method, body):
        """The reply message for a unary call, or None for an unknown one."""
        f = fields(body)
        key = f.get(1, [b""])[0]
        key = key.decode() if isinstance(key, bytes) else ""
        if method == "GetPlayerState":
            self.log({"call": method})
            with self.lock:
                return player_state(self.rev, self.current_row())
        if method == "GetFavorites":
            self.log({"call": method})
            with self.lock:
                # Favorites: refs 1, generation 2.
                return b"".join(f_str(1, k) for k in sorted(self.favorites)) + f_uint(2, self.rev)
        if method == "GetLyrics":
            self.log({"call": method, "key": key})
            return lyrics_message(self.lyrics) if key == self.track["key"] else b""
        if method == "GetSources":
            self.log({"call": method})
            # SourceList.sources 1; SourceInfo id 1, name 2, active 5,
            # capabilities 7; SourceCapabilities.dont_recommend 17.
            local = f_str(1, "local") + f_str(2, "Local")
            remote = (f_str(1, "ytmusic") + f_str(2, "YouTube Music") + f_uint(5, 1)
                      + f_bytes(7, f_uint(17, int(self.dont_recommend))))
            return f_bytes(1, local) + f_bytes(1, remote)
        if method == "SetFavorite":
            on = bool(f.get(2, [0])[0])
            self.log({"call": method, "key": key, "favorite": on})
            with self.lock:
                (self.favorites.add if on else self.favorites.discard)(key)
            threading.Timer(0.05, self.invalidate, (TABLE_FAVORITES,)).start()
            return b""
        if method == "DontRecommend":
            self.log({"call": method, "key": key})
            return b""
        if method == "Next":
            self.log({"call": method})
            # MutationResult.rev 1.
            return f_uint(1, self.rev)
        self.log({"call": method, "unknown": True})
        return None

    def serve(self, conn):
        try:
            self.connection(conn)
        except EOFError:
            pass
        except (OSError, ValueError, IndexError) as e:
            print(f"kopuzd-stub: {e}", file=sys.stderr, flush=True)
        finally:
            with self.lock:
                if conn in self.subscribers:
                    self.subscribers.remove(conn)
            conn.close()

    def connection(self, conn):
        buf = b""

        def read(n):
            nonlocal buf
            while len(buf) < n:
                chunk = conn.recv(65536)
                if not chunk:
                    raise EOFError
                buf += chunk
            out, buf = buf[:n], buf[n:]
            return out

        if read(len(PREFACE)) != PREFACE:
            raise ValueError("no HTTP/2 preface")
        self.send(conn, frame(SETTINGS, 0, 0))
        path, body = None, b""
        while True:
            head = read(9)
            length = int.from_bytes(head[:3], "big")
            kind, flags = head[3], head[4]
            stream = int.from_bytes(head[5:9], "big") & 0x7FFFFFFF
            payload = read(length)
            if kind == SETTINGS and not flags & ACK:
                self.send(conn, frame(SETTINGS, ACK, 0))
            elif kind == PING and not flags & ACK:
                self.send(conn, frame(PING, ACK, 0, payload))
            elif kind == GOAWAY:
                return
            elif kind == HEADERS and stream == 1:
                path = header_fields(payload).get(":path", "")
            elif kind == DATA and stream == 1:
                body += payload
            if kind in (HEADERS, DATA) and stream == 1 and flags & END_STREAM:
                self.dispatch(conn, path, body)

    def dispatch(self, conn, path, body):
        method = path.rsplit("/", 1)[-1]
        message = body[5:5 + int.from_bytes(body[1:5], "big")] if len(body) >= 5 else b""
        if method == "Subscribe":
            self.log({"call": method})
            self.send(conn, frame(HEADERS, END_HEADERS, 1, RESPONSE_HEADERS))
            with self.lock:
                self.subscribers.append(conn)
            return
        reply = self.answer(method, message)
        if reply is None:
            # Trailers-only, UNIMPLEMENTED: no message, so the call fails.
            block = RESPONSE_HEADERS + literal("grpc-status", "12")
            self.send(conn, frame(HEADERS, END_HEADERS | END_STREAM, 1, block))
            return
        self.send(conn, frame(HEADERS, END_HEADERS, 1, RESPONSE_HEADERS)
                     + frame(DATA, 0, 1, b"\x00" + struct.pack(">I", len(reply)) + reply)
                     + frame(HEADERS, END_HEADERS | END_STREAM, 1, literal("grpc-status", "0")))


def daemon(args):
    d = Daemon(args)
    os.makedirs(os.path.dirname(args.socket), mode=0o700, exist_ok=True)
    if os.path.exists(args.socket):
        os.unlink(args.socket)
    if not os.path.exists(args.control):
        os.mkfifo(args.control)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(args.socket)
    os.chmod(args.socket, 0o600)
    server.listen(16)
    threading.Thread(target=d.control, args=(args.control,), daemon=True).start()
    print(f"kopuzd-stub: listening on {args.socket}", flush=True)
    while True:
        conn, _ = server.accept()
        threading.Thread(target=d.serve, args=(conn,), daemon=True).start()


# ---------------- MPRIS ----------------

MPRIS_XML = """<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg direction="in" name="Offset" type="x"/></method>
    <method name="SetPosition">
      <arg direction="in" name="TrackId" type="o"/>
      <arg direction="in" name="Position" type="x"/>
    </method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="read"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"""


def player(args):
    import gi

    gi.require_version("GLib", "2.0")
    gi.require_version("Gio", "2.0")
    from gi.repository import Gio, GLib

    tracks = json.load(open(args.tracks))
    st = {"index": 0, "playing": True, "base": 0.0, "since": time.monotonic()}

    def position():
        p = st["base"] + (time.monotonic() - st["since"] if st["playing"] else 0.0)
        return int(min(p, tracks[st["index"]]["length"]) * 1_000_000)

    def metadata():
        t = tracks[st["index"]]
        m = {
            "mpris:trackid": GLib.Variant("o", f"/org/kopuz/track/{st['index'] + 1}"),
            "mpris:length": GLib.Variant("x", int(t["length"] * 1_000_000)),
            "xesam:title": GLib.Variant("s", t["title"]),
            "xesam:artist": GLib.Variant("as", [t["artist"]]),
            "xesam:album": GLib.Variant("s", t["album"]),
        }
        if t.get("url"):
            m["xesam:url"] = GLib.Variant("s", t["url"])
        return GLib.Variant("a{sv}", m)

    def props():
        n = st["index"] + 1 < len(tracks)
        return {
            "PlaybackStatus": GLib.Variant("s", "Playing" if st["playing"] else "Paused"),
            "Rate": GLib.Variant("d", 1.0),
            "MinimumRate": GLib.Variant("d", 1.0),
            "MaximumRate": GLib.Variant("d", 1.0),
            "Metadata": metadata(),
            "Position": GLib.Variant("x", position()),
            "CanGoNext": GLib.Variant("b", n),
            "CanGoPrevious": GLib.Variant("b", st["index"] > 0),
            "CanPlay": GLib.Variant("b", True),
            "CanPause": GLib.Variant("b", True),
            "CanSeek": GLib.Variant("b", False),
            "CanControl": GLib.Variant("b", True),
        }

    root = {
        "CanQuit": GLib.Variant("b", False),
        "CanRaise": GLib.Variant("b", False),
        "HasTrackList": GLib.Variant("b", False),
        "Identity": GLib.Variant("s", "Kopuz"),
        "DesktopEntry": GLib.Variant("s", "kopuz"),
        "SupportedUriSchemes": GLib.Variant("as", ["file"]),
        "SupportedMimeTypes": GLib.Variant("as", []),
    }

    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def changed(names):
        p = props()
        connection.emit_signal(
            None, "/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties", "PropertiesChanged",
            GLib.Variant("(sa{sv}as)", ("org.mpris.MediaPlayer2.Player", {k: p[k] for k in names}, [])),
        )

    def set_playing(on):
        st["base"] = position() / 1_000_000
        st["since"] = time.monotonic()
        st["playing"] = on
        changed(["PlaybackStatus"])

    def on_call(conn, sender, path, iface, method, params, invocation):
        with open(args.log, "a") as f:
            f.write(json.dumps({"mpris": method}) + "\n")
        if method == "PlayPause":
            set_playing(not st["playing"])
        elif method in ("Play", "Pause", "Stop"):
            set_playing(method == "Play")
        elif method == "Next" and st["index"] + 1 < len(tracks):
            st.update(index=st["index"] + 1, base=0.0, since=time.monotonic())
            changed(["Metadata", "CanGoNext", "CanGoPrevious", "PlaybackStatus"])
        invocation.return_value(None)

    def on_get(conn, sender, path, iface, name):
        return (root if iface == "org.mpris.MediaPlayer2" else props()).get(name)

    node = Gio.DBusNodeInfo.new_for_xml(MPRIS_XML)
    for iface in ("org.mpris.MediaPlayer2", "org.mpris.MediaPlayer2.Player"):
        connection.register_object("/org/mpris/MediaPlayer2", node.lookup_interface(iface), on_call, on_get, None)
    Gio.bus_own_name_on_connection(connection, args.name, Gio.BusNameOwnerFlags.NONE, None, None)
    print(f"kopuz-player-stub: owning {args.name}", flush=True)
    GLib.MainLoop().run()


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="mode", required=True)
    d = sub.add_parser("daemon")
    d.add_argument("--socket", required=True)
    d.add_argument("--control", required=True)
    d.add_argument("--log", required=True)
    d.add_argument("--lyrics", required=True)
    d.add_argument("--key", required=True)
    d.add_argument("--title", required=True)
    d.add_argument("--artist", required=True)
    d.add_argument("--album", required=True)
    d.add_argument("--length", type=float, required=True)
    p = sub.add_parser("player")
    p.add_argument("--name", default="org.mpris.MediaPlayer2.kopuz")
    p.add_argument("--tracks", required=True)
    p.add_argument("--log", required=True)
    args = ap.parse_args()
    daemon(args) if args.mode == "daemon" else player(args)


if __name__ == "__main__":
    main()
