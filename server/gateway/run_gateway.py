"""Console/frozen entry point for the gateway.

Used by the PyInstaller sidecar built by scripts/build-gateway.* (the desktop
app runs `whispr-gateway --host 127.0.0.1 --port 8765`). Also works directly:
`python run_gateway.py`. Configuration still comes from the WHISPR_* env vars
documented in app/main.py.
"""

import argparse
import ipaddress
import multiprocessing


def main() -> None:
    parser = argparse.ArgumentParser(description="Whispr STT gateway")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--log-level", default="info")
    parser.add_argument(
        "--allow-remote",
        action="store_true",
        help="allow binding outside loopback (unsafe without a reverse proxy and authentication)",
    )
    args = parser.parse_args()

    try:
        is_loopback = ipaddress.ip_address(args.host).is_loopback
    except ValueError:
        is_loopback = args.host.lower() == "localhost"
    if not is_loopback and not args.allow_remote:
        parser.error("refusing a non-loopback bind; pass --allow-remote explicitly")

    import uvicorn

    from app.main import app

    uvicorn.run(app, host=args.host, port=args.port, log_level=args.log_level)


if __name__ == "__main__":
    multiprocessing.freeze_support()  # no-op unless frozen on Windows
    main()
