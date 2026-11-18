"""A TCP proxy in front of a hub that logs each HTTP request line it
forwards, with the wall clock in ns, so a launch can be timed from the
outside the same way for both apps.

    python3 proxy.py LISTEN_PORT HUB_PORT LOG
"""
import asyncio
import re
import sys
import time

listen, target, path = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
REQ = re.compile(rb"(GET|POST|PUT|DELETE|PATCH) (\S+) HTTP/1\.[01]\r\n")
log = open(path, "a", buffering=1)


async def pipe(r, w, scan):
    try:
        while True:
            b = await r.read(65536)
            if not b:
                break
            if scan:
                for m in REQ.finditer(b):
                    log.write(f"{time.time_ns()} {m.group(1).decode()} {m.group(2).decode()}\n")
            w.write(b)
            await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def handle(cr, cw):
    try:
        sr, sw = await asyncio.open_connection("127.0.0.1", target)
    except Exception:
        cw.close()
        return
    await asyncio.gather(pipe(cr, sw, True), pipe(sr, cw, False))


async def main():
    server = await asyncio.start_server(handle, "127.0.0.1", listen)
    async with server:
        await server.serve_forever()


asyncio.run(main())
