"""Four concurrent ephemeral loopback servers, one GET each.

Run this when a test that starts its own server on 127.0.0.1 fails with
"error sending request" or `os error 10054`:

    python -I scripts/loopback-probe.py

Four `ok` lines mean loopback is healthy and the fault is in the code
under test. Any `client-error ... 10054` means something on this machine
is resetting concurrent loopback flows and no test using a local server
can be trusted. On this machine that was AdGuard's `adgnetworkwfpdrv` /
`adgnetworksockdrv` filters, which keep filtering while AdGuard's own
interface says protection is off; `Stop-Service 'Adguard Service'`
clears it. See "AdGuard resets concurrent loopback connections" in
docs/GOTCHAS.md (BUG-0039).

It deliberately uses nothing but the standard library, so a failure here
cannot be blamed on tokio, reqwest or anything of ours.
"""

import socket
import threading

RESULTS = []


def one(tag):
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.bind(("127.0.0.1", 0))
    srv.listen(8)
    port = srv.getsockname()[1]

    def serve():
        try:
            conn, _ = srv.accept()
        except OSError as e:
            RESULTS.append((tag, "accept-error", repr(e)))
            return
        try:
            conn.recv(4096)
            body = b'{}'
            conn.sendall(
                b"HTTP/1.1 200 X\r\ncontent-type: application/json\r\n"
                b"content-length: " + str(len(body)).encode() + b"\r\n"
                b"connection: close\r\n\r\n" + body
            )
        finally:
            conn.close()

    t = threading.Thread(target=serve, daemon=True)
    t.start()

    cli = socket.create_connection(("127.0.0.1", port), timeout=5)
    try:
        cli.sendall(b"GET /ping HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n")
        data = b""
        while True:
            chunk = cli.recv(4096)
            if not chunk:
                break
            data += chunk
        RESULTS.append((tag, "ok", data.split(b"\r\n")[0].decode()))
    except OSError as e:
        RESULTS.append((tag, "client-error", f"errno={e.errno} {e}"))
    finally:
        cli.close()
        srv.close()


threads = [threading.Thread(target=one, args=(i,)) for i in range(4)]
for t in threads:
    t.start()
for t in threads:
    t.join()

for tag, kind, detail in sorted(RESULTS):
    print(f"probe {tag}: {kind} — {detail}")
