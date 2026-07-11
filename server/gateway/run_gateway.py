"""Console/frozen entry point for the gateway.

Used by the PyInstaller sidecar built by scripts/build-gateway.* (the desktop
app runs `whispr-gateway --host 127.0.0.1 --port 8765`). Also works directly:
`python run_gateway.py`. Configuration still comes from the WHISPR_* env vars
documented in app/main.py.
"""

import argparse
import multiprocessing


def main() -> None:
    parser = argparse.ArgumentParser(description="Whispr STT gateway")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--log-level", default="info")
    args = parser.parse_args()

    import uvicorn

    from app.main import app

    uvicorn.run(app, host=args.host, port=args.port, log_level=args.log_level)


if __name__ == "__main__":
    multiprocessing.freeze_support()  # no-op unless frozen on Windows
    main()
