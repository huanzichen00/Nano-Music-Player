"""USB protocol and localhost forwarding tests; never contact a real phone."""

import contextlib
import importlib.util
import io
from pathlib import Path
import plistlib
import socket
import sys
import threading
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "iphone_transfer_usb.py"
SPEC = importlib.util.spec_from_file_location("iphone_transfer_usb", SCRIPT)
usb = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = usb
SPEC.loader.exec_module(usb)

TOKEN = "0123456789abcdef0123456789abcdef"
UDID = "00008140-001E18C914F1801C"
OTHER_UDID = "00008140-001E18C914F1801D"
PHONE_URL = "http://192.0.2.1:63139/#" + TOKEN


def frame(message, tag=1, version=1, kind=8):
    payload = plistlib.dumps(message)
    return usb.HEADER.pack(len(payload) + 16, version, kind, tag) + payload


def record(udid=UDID, device_id=4, connection_type="USB"):
    return {"DeviceID": device_id, "Properties": {
        "SerialNumber": udid, "ConnectionType": connection_type,
    }}


class ChunkedInput:
    def __init__(self, data, chunk=3):
        self.data = data
        self.chunk = chunk

    def recv(self, length):
        count = min(length, self.chunk)
        result, self.data = self.data[:count], self.data[count:]
        return result


def receive_all(connection):
    result = bytearray()
    while True:
        chunk = connection.recv(65536)
        if not chunk:
            return bytes(result)
        result.extend(chunk)


class FakeDaemon:
    """One socketpair per control connection, then raw HTTP after Connect."""

    def __init__(self, handler=None, records=None, result=0):
        self.records = records if records is not None else [record()]
        self.handler = handler
        self.result = result
        self.requests = []
        self.errors = []
        self.threads = []
        self.connections = []
        self.client_sockets = []
        self.client = usb.UsbmuxClient(timeout=1)
        self.client._open = self.open

    def open(self):
        client, daemon = socket.socketpair()
        client.settimeout(2)
        daemon.settimeout(2)
        self.client_sockets.append(client)
        self.connections.append(daemon)
        worker = threading.Thread(target=self.serve, args=(daemon,), daemon=True)
        self.threads.append(worker)
        worker.start()
        return client

    def serve(self, connection):
        try:
            first = usb.recv_plist(connection, 1)
            self.requests.append(first)
            if first["MessageType"] != "ListDevices":
                raise AssertionError("expected ListDevices")
            # Deliberately fragment the reply, including its 16-byte header.
            reply = frame({"DeviceList": self.records})
            for offset in range(0, len(reply), 7):
                connection.sendall(reply[offset:offset + 7])
            try:
                second = usb.recv_plist(connection, 2)
            except usb.TransferError:
                return  # ListDevices-only callers close the control socket.
            self.requests.append(second)
            if second["MessageType"] != "Connect":
                raise AssertionError("expected Connect")
            connection.sendall(frame({"MessageType": "Result", "Number": self.result}, tag=2))
            if self.result == 0 and self.handler is not None:
                self.handler(connection)
        except Exception as error:
            self.errors.append(error)
        finally:
            connection.close()

    def close(self):
        for connection in self.connections:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            connection.close()
        for worker in self.threads:
            worker.join(2)


class ProtocolTests(unittest.TestCase):
    def test_partial_reads_leave_following_stream_bytes_untouched(self):
        connection = ChunkedInput(frame({"Number": 0}) + b"HTTP/1.1", chunk=2)
        self.assertEqual(usb.recv_plist(connection, 1), {"Number": 0})
        self.assertEqual(connection.data, b"HTTP/1.1")

    def test_eof_in_header_or_payload_is_rejected(self):
        message = frame({"Number": 0})
        for count in (0, 7, 16, len(message) - 1):
            with self.subTest(count=count), self.assertRaises(usb.TransferError):
                usb.recv_plist(ChunkedInput(message[:count]), 1)

    def test_packet_lengths_are_bounded_before_reading_payload(self):
        for length in (0, 15, usb.MAX_PACKET + 1, 0xFFFFFFFF):
            with self.subTest(length=length), self.assertRaises(usb.TransferError):
                usb.recv_plist(ChunkedInput(usb.HEADER.pack(length, 1, 8, 1)), 1)

    def test_protocol_message_and_tag_must_match(self):
        for options in ({"version": 0}, {"kind": 6}, {"tag": 2}):
            with self.subTest(options=options), self.assertRaises(usb.TransferError):
                usb.recv_plist(ChunkedInput(frame({}, **options)), 1)

    def test_invalid_plist_is_redacted(self):
        payload = TOKEN.encode()
        malformed = usb.HEADER.pack(16 + len(payload), 1, 8, 1) + payload
        with self.assertRaises(usb.TransferError) as caught:
            usb.recv_plist(ChunkedInput(malformed), 1)
        self.assertNotIn(TOKEN, str(caught.exception))
        with self.assertRaises(usb.TransferError):
            usb.recv_plist(ChunkedInput(frame([1, 2])), 1)

    def test_only_usb_devices_are_eligible(self):
        devices = usb.usb_devices({"DeviceList": [record(connection_type="Network"),
                                                  record(OTHER_UDID, 9),
                                                  record(device_id=True)]})
        self.assertEqual(devices, [usb.UsbDevice(9, OTHER_UDID)])
        with self.assertRaises(usb.TransferError):
            usb.select_device(devices, UDID)
        with self.assertRaises(usb.TransferError):
            usb.select_device([usb.UsbDevice(1, UDID), usb.UsbDevice(2, OTHER_UDID)])

    def test_connect_uses_network_byte_order_and_preserves_stream(self):
        daemon = FakeDaemon(handler=lambda connection: connection.sendall(b"HTTP response"))
        self.addCleanup(daemon.close)
        with daemon.client.connect(UDID, 63139) as tunnel:
            self.assertEqual(receive_all(tunnel), b"HTTP response")
        self.assertEqual(daemon.requests[1]["DeviceID"], 4)
        self.assertEqual(daemon.requests[1]["PortNumber"], socket.htons(63139))
        self.assertFalse(daemon.errors)

    def test_selected_udid_is_checked_again_when_device_ids_change(self):
        daemon = FakeDaemon()
        self.addCleanup(daemon.close)
        selected = usb.select_device(daemon.client.devices())
        daemon.records = [record(OTHER_UDID, 4), record(UDID, 23)]
        with daemon.client.connect(selected.udid, 63139):
            pass
        self.assertEqual(daemon.requests[-1]["DeviceID"], 23)
        daemon.records = [record(OTHER_UDID, 4), record(UDID, 23, "Network")]
        with self.assertRaises(usb.TransferError):
            daemon.client.connect(selected.udid, 63139)
        self.assertEqual(daemon.client_sockets[-1].fileno(), -1)

    def test_connect_errors_close_the_socket(self):
        for result in (3, True, "0"):
            with self.subTest(result=result):
                daemon = FakeDaemon(result=result)
                self.addCleanup(daemon.close)
                with self.assertRaises(usb.TransferError):
                    daemon.client.connect(UDID, 63139)
                self.assertEqual(daemon.client_sockets[-1].fileno(), -1)

    def test_socket_is_closed_if_unix_connect_fails(self):
        connection = mock.Mock()
        connection.connect.side_effect = OSError("secret " + TOKEN)
        with mock.patch.object(usb.socket, "socket", return_value=connection):
            with self.assertRaises(usb.TransferError) as caught:
                usb.UsbmuxClient().devices()
        connection.close.assert_called_once()
        self.assertNotIn(TOKEN, str(caught.exception))


class AddressTests(unittest.TestCase):
    def test_full_phone_address_parses_without_secret_repr(self):
        address = usb.parse_phone_url(PHONE_URL)
        self.assertEqual(address.port, 63139)
        self.assertEqual(address.token, TOKEN)
        self.assertNotIn(TOKEN, repr(address))
        self.assertEqual(usb.parse_phone_url("http://[::1]:23/#" + TOKEN).port, 23)

    def test_missing_or_unsafe_address_is_rejected_without_echo(self):
        for value in ("https://phone:23/#" + TOKEN, "http://phone/#" + TOKEN,
                      "http://phone:0/#" + TOKEN, "http://phone:65536/#" + TOKEN,
                      "http://user:password@phone:23/#" + TOKEN,
                      "http://phone:23/other#" + TOKEN,
                      "http://phone:23/?query=1#" + TOKEN,
                      "http://phone:23/#" + TOKEN + "\r\nLocation: attack",
                      "http://phone:23/#short", "http://phone:23/",
                      "http://phone:23/#" + TOKEN + " " * 3000):
            with self.subTest(value=value[:24]), self.assertRaises(usb.TransferError) as caught:
                usb.parse_phone_url(value)
            self.assertNotIn(TOKEN, str(caught.exception))

    def test_cli_errors_never_echo_secret_arguments(self):
        output = io.StringIO()
        with contextlib.redirect_stderr(output), self.assertRaises(SystemExit):
            usb.main(["--bad-option", PHONE_URL])
        self.assertNotIn(TOKEN, output.getvalue())
        with contextlib.redirect_stderr(output):
            self.assertEqual(usb.main([PHONE_URL.replace(":63139", ":70000")]), 1)
        self.assertNotIn(TOKEN, output.getvalue())

    def test_stdin_cli_opens_only_token_free_local_address(self):
        output = io.StringIO()
        mux = mock.Mock()
        mux.devices.return_value = [usb.UsbDevice(4, UDID)]
        server = mock.MagicMock()
        server.__enter__.return_value = server
        server.local_url = "http://127.0.0.1:8765/"
        with (mock.patch.object(usb.sys, "stdin", io.StringIO(PHONE_URL + "\n")),
              mock.patch.object(usb, "UsbmuxClient", return_value=mux),
              mock.patch.object(usb, "UsbProxyServer", return_value=server) as constructor,
              mock.patch.object(usb.webbrowser, "open") as open_browser,
              contextlib.redirect_stdout(output)):
            self.assertEqual(usb.main(["--url-stdin", "--listen-port", "8765", "--open"]), 0)
        open_browser.assert_called_once_with(server.local_url)
        self.assertEqual(constructor.call_args.args[:3], (8765, usb.parse_phone_url(PHONE_URL), UDID))
        self.assertNotIn(TOKEN, output.getvalue())


class ProxyTests(unittest.TestCase):
    def start_proxy(self, daemon=None, mux=None, idle_timeout=2):
        self.log = io.StringIO()
        if daemon is not None:
            self.addCleanup(daemon.close)
            mux = daemon.client
        server = usb.UsbProxyServer(0, usb.parse_phone_url(PHONE_URL), UDID, mux,
                                    error_stream=self.log, idle_timeout=idle_timeout)
        worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
        worker.start()

        def stop():
            server.shutdown()
            server.server_close()
            worker.join(2)

        self.addCleanup(stop)
        self.assertEqual(server.server_address[0], "127.0.0.1")
        self.assertNotIn(TOKEN, server.local_url)
        return server

    def request(self, server, request, half_close=False):
        with socket.create_connection(server.server_address, timeout=3) as connection:
            connection.sendall(request)
            if half_close:
                connection.shutdown(socket.SHUT_WR)
            return receive_all(connection)

    def test_bootstrap_redirect_preserves_fragment_without_contacting_phone(self):
        mux = mock.Mock()
        server = self.start_proxy(mux=mux)
        response = self.request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        self.assertIn(b"302 Found", response)
        self.assertIn(b"Location: " + usb.PAIRED_PATH + b"#" + TOKEN.encode(), response)
        self.assertIn(b"Cache-Control: no-store", response)
        mux.connect.assert_not_called()
        self.assertNotIn(TOKEN, self.log.getvalue())

    def test_paired_page_rewrites_only_the_request_path(self):
        requests = []

        def phone(connection):
            parts, rest = usb.read_http_request(connection)
            requests.append((parts, rest))
            connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\npage")

        daemon = FakeDaemon(phone)
        server = self.start_proxy(daemon)
        response = self.request(server, b"GET " + usb.PAIRED_PATH + b" HTTP/1.1\r\nHost: localhost\r\n\r\n")
        self.assertTrue(response.endswith(b"page"))
        self.assertEqual(requests[0][0], [b"GET", b"/", b"HTTP/1.1"])
        self.assertFalse(daemon.errors)

    def test_binary_upload_and_authenticated_api_forward_unchanged(self):
        body = bytes(range(256)) * 1700
        received = []

        def phone(connection):
            parts, remainder = usb.read_http_request(connection)
            headers, initial_body = remainder.split(b"\r\n\r\n", 1)
            upload = bytearray(initial_body)
            while len(upload) < len(body):
                upload.extend(connection.recv(min(997, len(body) - len(upload))))
            received.append((parts, headers, bytes(upload)))
            connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")

        daemon = FakeDaemon(phone)
        server = self.start_proxy(daemon)
        request = (b"POST /api/upload HTTP/1.1\r\nHost: localhost\r\nX-Nano-Token: "
                   + TOKEN.encode() + b"\r\nContent-Length: " + str(len(body)).encode()
                   + b"\r\nX-Nano-Filename: test.wav\r\n\r\n" + body)
        response = self.request(server, request, half_close=True)
        self.assertTrue(response.endswith(b"ok"))
        self.assertEqual(received[0][0][1], b"/api/upload")
        self.assertIn(b"X-Nano-Token: " + TOKEN.encode(), received[0][1])
        self.assertEqual(received[0][2], body)
        self.assertFalse(daemon.errors)
        self.assertNotIn(TOKEN, self.log.getvalue())

    def test_worker_failure_is_redacted_and_other_connections_still_work(self):
        mux = mock.Mock()
        mux.connect.side_effect = RuntimeError("do not print " + TOKEN)
        server = self.start_proxy(mux=mux)
        response = self.request(server, b"GET /api/library HTTP/1.1\r\nHost: localhost\r\n\r\n")
        self.assertIn(b"502 Bad Gateway", response)
        self.assertNotIn(TOKEN.encode(), response)
        self.assertNotIn(TOKEN, self.log.getvalue())
        retry = self.request(server, b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        self.assertIn(b"302 Found", retry)

    def test_bad_or_oversized_requests_never_open_a_tunnel(self):
        mux = mock.Mock()
        server = self.start_proxy(mux=mux)
        for request in (b"GET http://elsewhere/ HTTP/1.1\r\n\r\n",
                        b"CONNECT elsewhere:80 HTTP/1.1\r\n\r\n",
                        b"GET //elsewhere/ HTTP/1.1\r\n\r\n"):
            with self.subTest(length=len(request)):
                response = self.request(server, request)
                self.assertIn(b"502 Bad Gateway", response)
        # Closing a socket with unread oversized input may produce TCP RST;
        # either closure form is valid, and no USB tunnel may be opened.
        with socket.create_connection(server.server_address, timeout=2) as connection:
            try:
                connection.sendall(b"GET / HTTP/1.1\r\nSecret: " + TOKEN.encode()
                                   + b"x" * usb.MAX_HTTP_HEADER)
                receive_all(connection)
            except (ConnectionResetError, BrokenPipeError):
                pass
        mux.connect.assert_not_called()
        self.assertNotIn(TOKEN, self.log.getvalue())

    def test_shutdown_closes_connected_browser_and_usb_stream(self):
        accepted = threading.Event()
        closed = threading.Event()

        def phone(connection):
            usb.read_http_request(connection)
            accepted.set()
            if connection.recv(1) == b"":
                closed.set()

        server = self.start_proxy(FakeDaemon(phone))
        with socket.create_connection(server.server_address, timeout=2) as browser:
            browser.sendall(b"GET /api/library HTTP/1.1\r\nHost: localhost\r\n\r\n")
            self.assertTrue(accepted.wait(2))
            server.server_close()
            self.assertEqual(browser.recv(1), b"")
            self.assertTrue(closed.wait(2))

    def test_idle_stream_expires_without_leaking_credentials(self):
        def phone(connection):
            usb.read_http_request(connection)
            connection.recv(1)

        server = self.start_proxy(FakeDaemon(phone), idle_timeout=0.05)
        self.assertEqual(self.request(server, b"GET /api/library HTTP/1.1\r\nHost: localhost\r\n\r\n"), b"")
        self.assertIn("interrupted", self.log.getvalue())
        self.assertNotIn(TOKEN, self.log.getvalue())


class ShortSocket:
    """Force partial reads/writes even when the OS socket has ample capacity."""

    def __init__(self, connection):
        self.connection = connection

    def fileno(self):
        return self.connection.fileno()

    def setblocking(self, value):
        self.connection.setblocking(value)

    def recv(self, count):
        return self.connection.recv(min(count, 997))

    def send(self, data):
        return self.connection.send(data[:113])

    def shutdown(self, how):
        self.connection.shutdown(how)


class RelayTests(unittest.TestCase):
    def test_partial_writes_and_both_half_closes_preserve_bytes(self):
        browser, left = socket.socketpair()
        right, phone = socket.socketpair()
        for connection in (browser, left, right, phone):
            connection.settimeout(3)
            self.addCleanup(connection.close)
        payload = bytes(range(256)) * 2000
        reply = b"reply\x00" * 25000
        errors = []

        def forward():
            try:
                usb.relay(ShortSocket(left), ShortSocket(right), threading.Event(), idle_timeout=2)
            except Exception as error:
                errors.append(error)
            finally:
                left.close()
                right.close()

        def respond():
            try:
                self.assertEqual(receive_all(phone), payload)
                phone.sendall(reply)
                phone.shutdown(socket.SHUT_WR)
            except Exception as error:
                errors.append(error)

        relay_worker = threading.Thread(target=forward, daemon=True)
        responder = threading.Thread(target=respond, daemon=True)
        relay_worker.start()
        responder.start()
        browser.sendall(payload)
        browser.shutdown(socket.SHUT_WR)
        self.assertEqual(receive_all(browser), reply)
        relay_worker.join(3)
        responder.join(3)
        self.assertFalse(relay_worker.is_alive())
        self.assertFalse(responder.is_alive())
        self.assertFalse(errors, errors)


if __name__ == "__main__":
    unittest.main()
