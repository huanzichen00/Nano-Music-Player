#!/usr/bin/env python3
"""Expose Nano's iPhone transfer page on localhost through USB, using stdlib only.

Protocol reference: https://github.com/libimobiledevice/libusbmuxd (iproxy).
This is an independent implementation; it neither installs software on the phone
nor starts the app. Keep Computer Transfer enabled on the connected iPhone.
"""

import argparse
from dataclasses import dataclass, field
import getpass
import plistlib
import re
import selectors
import socket
import socketserver
import struct
import sys
import threading
import time
from urllib.parse import urlsplit
import webbrowser


HEADER = struct.Struct("<IIII")
MAX_PACKET = 1024 * 1024
MAX_HTTP_HEADER = 16 * 1024
BUFFER_SIZE = 128 * 1024
PAIRED_PATH = b"/__nano_usb_transfer__/"
USBMUX_SOCKET = "/var/run/usbmuxd"


class TransferError(Exception):
    """A safe, user-facing error that never includes URLs or request contents."""


@dataclass(frozen=True)
class PhoneAddress:
    port: int
    token: str = field(repr=False)


@dataclass(frozen=True)
class UsbDevice:
    device_id: int
    udid: str


def parse_phone_url(value):
    """Accept the current app's full URL, without reflecting invalid input."""
    message = "Use the complete phone address: http://phone-address:port/#pairing-token."
    if not isinstance(value, str) or len(value) > 2048:
        raise TransferError(message)
    if any(ord(character) <= 32 or ord(character) == 127 for character in value):
        raise TransferError(message)
    try:
        parts = urlsplit(value)
        port = parts.port
        valid = (
            parts.scheme == "http"
            and parts.hostname
            and parts.username is None
            and parts.password is None
            and parts.path in ("", "/")
            and not parts.query
            and port is not None
            and 1 <= port <= 65535
            and re.fullmatch(r"[a-f0-9]{32}", parts.fragment)
        )
    except ValueError:
        valid = False
    if not valid:
        raise TransferError(message)
    return PhoneAddress(port, parts.fragment)


def recv_exact(connection, length):
    """Read one bounded frame component, leaving subsequent TCP bytes intact."""
    if length < 0 or length > MAX_PACKET:
        raise TransferError("Invalid usbmuxd packet length.")
    chunks = bytearray()
    while len(chunks) < length:
        chunk = connection.recv(length - len(chunks))
        if not chunk:
            raise TransferError("usbmuxd closed the connection during its reply.")
        chunks.extend(chunk)
    return bytes(chunks)


def send_plist(connection, tag, message):
    payload = plistlib.dumps(message, fmt=plistlib.FMT_XML)
    if len(payload) + HEADER.size > MAX_PACKET:
        raise TransferError("usbmuxd request is too large.")
    connection.sendall(HEADER.pack(len(payload) + HEADER.size, 1, 8, tag) + payload)


def recv_plist(connection, expected_tag):
    length, version, message, tag = HEADER.unpack(recv_exact(connection, HEADER.size))
    if length < HEADER.size or length > MAX_PACKET:
        raise TransferError("Invalid usbmuxd packet length.")
    if version != 1 or message != 8 or tag != expected_tag:
        raise TransferError("Unexpected usbmuxd reply header.")
    payload = recv_exact(connection, length - HEADER.size)
    try:
        result = plistlib.loads(payload)
    except Exception:
        # XML parser errors vary between Python versions. Do not reflect their
        # text: malformed daemon replies need never appear in terminal logs.
        raise TransferError("Invalid usbmuxd property-list reply.") from None
    if not isinstance(result, dict):
        raise TransferError("Invalid usbmuxd property-list reply.")
    return result


def usb_devices(reply):
    records = reply.get("DeviceList")
    if not isinstance(records, list) or len(records) > 4096:
        raise TransferError("usbmuxd did not return a device list.")
    devices = []
    for record in records:
        if not isinstance(record, dict):
            continue
        properties = record.get("Properties")
        device_id = record.get("DeviceID")
        if not isinstance(properties, dict) or properties.get("ConnectionType") != "USB":
            continue
        udid = properties.get("SerialNumber")
        if (type(device_id) is int and 0 < device_id <= 0xFFFFFFFF
                and isinstance(udid, str) and re.fullmatch(r"[a-fA-F0-9-]{20,80}", udid)):
            devices.append(UsbDevice(device_id, udid))
    return devices


def select_device(devices, udid=None):
    if udid is not None:
        matches = [device for device in devices if device.udid.lower() == udid.lower()]
        if len(matches) != 1:
            raise TransferError("The selected iPhone is not connected through USB.")
        return matches[0]
    if not devices:
        raise TransferError("No USB iPhone found. Connect and unlock the phone, then trust this computer.")
    if len(devices) != 1:
        raise TransferError("Several USB devices are connected; select one with --udid.")
    return devices[0]


class UsbmuxClient:
    def __init__(self, socket_path=USBMUX_SOCKET, timeout=10):
        self.socket_path = socket_path
        self.timeout = timeout

    def _open(self):
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            connection.settimeout(self.timeout)
            connection.connect(self.socket_path)
            return connection
        except BaseException:
            connection.close()
            raise

    @staticmethod
    def _request(connection, tag, message_type, **values):
        send_plist(connection, tag, {
            "MessageType": message_type,
            "ClientVersionString": "nano-usb-transfer-1",
            "ProgName": "nano-usb-transfer",
            "kLibUSBMuxVersion": 3,
            **values,
        })
        return recv_plist(connection, tag)

    def devices(self):
        try:
            with self._open() as connection:
                return usb_devices(self._request(connection, 1, "ListDevices"))
        except OSError:
            raise TransferError("Cannot reach usbmuxd. This tool needs a Mac with the iPhone connected by USB.") from None

    def connect(self, udid, port):
        """Resolve the same USB UDID on every connection; never switch phones."""
        if type(port) is not int or not 1 <= port <= 65535:
            raise TransferError("Invalid phone transfer port.")
        connection = None
        try:
            connection = self._open()
            device = select_device(usb_devices(self._request(connection, 1, "ListDevices")), udid)
            reply = self._request(connection, 2, "Connect", DeviceID=device.device_id,
                                  PortNumber=socket.htons(port))
            number = reply.get("Number")
            if reply.get("MessageType") != "Result" or type(number) is not int or not 0 <= number <= 0xFFFFFFFF:
                raise TransferError("Invalid usbmuxd connect result.")
            if number != 0:
                raise TransferError("USB could not connect to the app. Keep Computer Transfer open and use its current address.")
            connection.settimeout(self.timeout)
            return connection
        except BaseException as error:
            if connection is not None:
                connection.close()
            if isinstance(error, OSError):
                raise TransferError("USB connection failed or timed out. Check the cable and keep Computer Transfer open.") from None
            raise


def read_http_request(connection):
    data = bytearray()
    while True:
        marker = data.find(b"\r\n\r\n")
        if marker >= 0:
            break
        if len(data) >= MAX_HTTP_HEADER:
            raise TransferError("HTTP request headers are too large.")
        chunk = connection.recv(min(4096, MAX_HTTP_HEADER - len(data)))
        if not chunk:
            raise TransferError("Browser closed the request.")
        data.extend(chunk)
    line_end = data.find(b"\r\n")
    parts = data[:line_end].split(b" ")
    if (len(parts) != 3 or parts[0] not in (b"GET", b"POST", b"HEAD")
            or parts[2] not in (b"HTTP/1.0", b"HTTP/1.1")
            or not parts[1].startswith(b"/") or parts[1].startswith(b"//")
            or len(parts[1]) > 2048
            or any(byte < 33 or byte > 126 for byte in parts[1])
            or b"#" in parts[1]):
        raise TransferError("Unsupported browser request.")
    return parts, bytes(data[line_end:])


def send_response(connection, status, body=b"", extra_headers=b""):
    connection.sendall(
        b"HTTP/1.1 " + status + b"\r\nContent-Type: text/plain; charset=utf-8\r\n"
        + b"Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n"
        + b"Connection: close\r\nContent-Length: " + str(len(body)).encode("ascii")
        + b"\r\n" + extra_headers + b"\r\n" + body
    )


def relay(left, right, stopped, initial=b"", idle_timeout=60):
    """Bounded bidirectional stream, including partial writes and half closes."""
    peers = {left: right, right: left}
    pending = {left: bytearray(), right: bytearray(initial)}
    reading = {left: True, right: True}
    writing = {left: True, right: True}
    last_activity = time.monotonic()
    left.setblocking(False)
    right.setblocking(False)
    with selectors.DefaultSelector() as selector:
        while not stopped.is_set():
            for connection, peer in peers.items():
                if not reading[peer] and not pending[connection] and writing[connection]:
                    connection.shutdown(socket.SHUT_WR)
                    writing[connection] = False
                events = 0
                if reading[connection] and len(pending[peer]) < BUFFER_SIZE:
                    events |= selectors.EVENT_READ
                if pending[connection]:
                    events |= selectors.EVENT_WRITE
                if connection in selector.get_map():
                    if events:
                        selector.modify(connection, events)
                    else:
                        selector.unregister(connection)
                elif events:
                    selector.register(connection, events)
            if not selector.get_map():
                return
            remaining = idle_timeout - (time.monotonic() - last_activity)
            if remaining <= 0:
                raise TransferError("Phone transfer timed out. Return to the app and retry.")
            for key, events in selector.select(min(remaining, 0.5)):
                connection = key.fileobj
                peer = peers[connection]
                if events & selectors.EVENT_READ:
                    try:
                        chunk = connection.recv(min(64 * 1024, BUFFER_SIZE - len(pending[peer])))
                    except BlockingIOError:
                        chunk = None
                    if chunk == b"":
                        reading[connection] = False
                    elif chunk:
                        pending[peer].extend(chunk)
                        last_activity = time.monotonic()
                if events & selectors.EVENT_WRITE:
                    try:
                        count = connection.send(pending[connection])
                    except BlockingIOError:
                        continue
                    if count == 0:
                        raise TransferError("USB stream closed while sending.")
                    del pending[connection][:count]
                    last_activity = time.monotonic()


class ProxyHandler(socketserver.BaseRequestHandler):
    def handle(self):
        upstream = None
        relaying = False
        try:
            self.request.settimeout(10)
            parts, remainder = read_http_request(self.request)
            method, path, version = parts
            if path == b"/" and method in (b"GET", b"HEAD"):
                # Opening the printed token-free localhost address pairs only
                # this browser. The secret is never placed in terminal output.
                send_response(self.request, b"302 Found", extra_headers=(
                    b"Location: " + PAIRED_PATH + b"#"
                    + self.server.phone.token.encode("ascii") + b"\r\n"
                ))
                return
            if path == PAIRED_PATH:
                path = b"/"
            upstream = self.server.mux.connect(self.server.udid, self.server.phone.port)
            if not self.server.track(upstream):
                return
            relaying = True
            relay(self.request, upstream, self.server.stopped,
                  b" ".join((method, path, version)) + remainder,
                  self.server.idle_timeout)
        except Exception:
            # Neither request headers nor arbitrary exception text are logged;
            # either can contain pairing credentials. Other clients can retry.
            if not self.server.stopped.is_set():
                self.server.report_error()
                if not relaying:
                    try:
                        send_response(self.request, b"502 Bad Gateway", (
                            b"The USB phone connection is unavailable. Keep Computer Transfer "
                            b"open on the iPhone and retry with its current address.\n"
                        ))
                    except OSError:
                        pass
        finally:
            if upstream is not None:
                self.server.untrack(upstream)
                upstream.close()


class UsbProxyServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True
    block_on_close = False

    def __init__(self, port, phone, udid, mux, error_stream=None, idle_timeout=60):
        self.phone = phone
        self.udid = udid
        self.mux = mux
        self.idle_timeout = idle_timeout
        self.error_stream = error_stream if error_stream is not None else sys.stderr
        self.stopped = threading.Event()
        self._connections = set()
        self._lock = threading.Lock()
        self._slots = threading.BoundedSemaphore(16)
        # This address is intentionally fixed; there is no LAN/public bind flag.
        super().__init__(("127.0.0.1", port), ProxyHandler)

    @property
    def local_url(self):
        return "http://127.0.0.1:{}/".format(self.server_address[1])

    def track(self, connection):
        with self._lock:
            if self.stopped.is_set():
                connection.close()
                return False
            self._connections.add(connection)
            return True

    def untrack(self, connection):
        with self._lock:
            self._connections.discard(connection)

    def process_request(self, request, client_address):
        if not self._slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        if not self.track(request):
            self._slots.release()
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.untrack(request)
            self._slots.release()
            self.shutdown_request(request)
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.untrack(request)
            self._slots.release()

    def report_error(self):
        try:
            print("USB request interrupted; keep Computer Transfer open on the iPhone and retry.",
                  file=self.error_stream, flush=True)
        except (OSError, ValueError):
            pass

    def handle_error(self, request, client_address):
        self.report_error()

    def server_close(self):
        self.stopped.set()
        with self._lock:
            connections = list(self._connections)
        for connection in connections:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            connection.close()
        super().server_close()


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse's default message may echo a mistyped secret URL argument.
        self.exit(2, "Invalid options. Use --help for usage.\n")


def main(argv=None):
    parser = PrivateArgumentParser(description=__doc__)
    parser.add_argument("url", nargs="?", help="complete phone transfer URL (omit for a hidden prompt)")
    parser.add_argument("--url-stdin", action="store_true", help="read the URL from stdin instead of the terminal")
    parser.add_argument("--udid", help="select one USB device by UDID; otherwise require exactly one")
    parser.add_argument("--listen-port", type=int, default=0, help="localhost port, default: automatically chosen")
    parser.add_argument("--socket", default=USBMUX_SOCKET, help="usbmuxd Unix socket path")
    parser.add_argument("--open", action="store_true", help="open the local page in the default browser")
    args = parser.parse_args(argv)
    try:
        if args.url_stdin and args.url is not None:
            raise TransferError("Supply the address either as an argument or with --url-stdin.")
        if not 0 <= args.listen_port <= 65535:
            raise TransferError("Local port must be between 0 and 65535.")
        if args.url_stdin:
            value = sys.stdin.readline(2050).rstrip("\r\n")
        elif args.url is not None:
            value = args.url
        elif sys.stdin.isatty():
            value = getpass.getpass("Paste the complete phone transfer URL (hidden): ")
        else:
            raise TransferError("Provide a phone address or use --url-stdin.")
        phone = parse_phone_url(value)
        mux = UsbmuxClient(args.socket)
        device = select_device(mux.devices(), args.udid)
        with UsbProxyServer(args.listen_port, phone, device.udid, mux) as server:
            print("USB transfer: " + server.local_url, flush=True)
            print("Keep Computer Transfer open on the iPhone. Ctrl-C stops this local proxy.", flush=True)
            if args.open:
                webbrowser.open(server.local_url)
            try:
                server.serve_forever(poll_interval=0.2)
            except KeyboardInterrupt:
                pass
        return 0
    except TransferError as error:
        print(str(error), file=sys.stderr)
        return 1
    except (OSError, EOFError):
        # OS errors may reflect paths supplied by the caller. Keep logs safe.
        print("USB transfer could not start. Check the full phone address, USB connection, "
              "selected device and local port; keep Computer Transfer open.", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
