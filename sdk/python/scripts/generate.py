"""Generates the modules of `feld` that come from the daemon API's OpenAPI definition.

    uv run python scripts/generate.py

`openapi-python-client` writes the package into a temporary directory, and this copies the
generated parts into `src/feld`, beside the hand-written modules.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OPENAPI = ROOT.parent.parent / "docs" / "guide" / "daemon-api.openapi.yaml"
PACKAGE = ROOT / "src" / "feld"
GENERATED_DIRS = ["api", "models"]
GENERATED_FILES = ["client.py", "errors.py", "types.py"]


def main() -> None:
    with tempfile.TemporaryDirectory() as temp:
        output = Path(temp) / "feld"
        subprocess.run(
            [
                sys.executable,
                "-m",
                "openapi_python_client",
                "generate",
                "--path",
                str(OPENAPI),
                "--config",
                str(ROOT / "openapi-python-client.yaml"),
                "--custom-template-path",
                str(ROOT / "templates"),
                "--meta",
                "none",
                "--output-path",
                str(output),
                "--fail-on-warning",
            ],
            check=True,
            cwd=temp,
        )
        for name in GENERATED_DIRS:
            shutil.rmtree(PACKAGE / name, ignore_errors=True)
            shutil.copytree(output / name, PACKAGE / name)
        for name in GENERATED_FILES:
            shutil.copy2(output / name, PACKAGE / name)


if __name__ == "__main__":
    main()
