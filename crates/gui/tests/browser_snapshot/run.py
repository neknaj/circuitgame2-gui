"""配布用の WASM を、同じバイナリを使う Chrome のスモークテストで検査する。"""

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from queue import Empty, SimpleQueue
import shutil
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread
from time import monotonic


def run_browser(site, chrome):
    results = SimpleQueue()

    class SmokeHandler(SimpleHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            if self.path != "/snapshot-result" or not 1 <= length <= 4096:
                self.send_error(400)
                return
            result = self.rfile.read(length).decode("utf-8", errors="replace")
            self.send_response(204)
            self.end_headers()
            results.put(result)

    fixtures = Path(__file__).resolve().parent
    with TemporaryDirectory(prefix="ncg2-browser-snapshot-") as directory:
        workspace = Path(directory)
        webroot = workspace / "site"
        shutil.copytree(site, webroot)
        for name in ("smoke.html", "cli-snapshot.bin"):
            shutil.copyfile(fixtures / name, webroot / name)

        handler = partial(SmokeHandler, directory=webroot)
        with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
            thread = Thread(target=server.serve_forever, daemon=True)
            thread.start()
            url = f"http://127.0.0.1:{server.server_port}/smoke.html"
            log_path = workspace / "chrome.log"
            try:
                with log_path.open("wb") as log:
                    with subprocess.Popen([
                        chrome,
                        "--headless",
                        "--no-first-run",
                        "--no-default-browser-check",
                        "--disable-background-networking",
                        "--use-gl=angle",
                        "--use-angle=swiftshader",
                        f"--user-data-dir={workspace / 'profile'}",
                        url,
                    ], stdout=log, stderr=subprocess.STDOUT) as browser:
                        try:
                            deadline = monotonic() + 45
                            while monotonic() < deadline:
                                try:
                                    return results.get(timeout=1)
                                except Empty:
                                    if browser.poll() is not None:
                                        return f"FAIL: Chrome exited with status {browser.returncode}."
                            return "FAIL: The browser did not report a result within 45 seconds."
                        finally:
                            if browser.poll() is None:
                                browser.terminate()
                                try:
                                    browser.wait(timeout=5)
                                except subprocess.TimeoutExpired:
                                    browser.kill()
            finally:
                server.shutdown()
                thread.join()
                if log_path.exists():
                    diagnostics = log_path.read_text(encoding="utf-8", errors="replace").strip()
                    if diagnostics:
                        print(diagnostics[-4000:])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("site", type=Path, help="assembled Pages site directory")
    args = parser.parse_args()
    chrome = shutil.which("google-chrome")
    if chrome is None:
        raise SystemExit("The browser smoke test needs the runner's installed google-chrome.")
    result = run_browser(args.site, chrome)
    if result != "PASS":
        raise SystemExit(result)
    print("The CLI snapshot survived a Chrome WASM round trip and both invalid lengths were rejected.")


if __name__ == "__main__":
    main()
