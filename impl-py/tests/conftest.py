"""Pytest configuration.

Ensures the ``mty`` package is importable when the test suite is
invoked from the workspace root (``python -m pytest impl-py/tests/``).
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

# tests/ lives next to mty/. Add the parent (impl-py/) to sys.path so
# ``import mty`` resolves whether pytest is invoked from impl-py/ or
# from the workspace root.
HERE = Path(__file__).resolve().parent
IMPL_PY = HERE.parent
if str(IMPL_PY) not in sys.path:
    sys.path.insert(0, str(IMPL_PY))

# Examples directory lives at <workspace>/examples; expose its path via
# a pytest fixture below.
WORKSPACE = IMPL_PY.parent
EXAMPLES_DIR = WORKSPACE / "examples"


import pytest


@pytest.fixture(scope="session")
def examples_dir() -> Path:
    return EXAMPLES_DIR


@pytest.fixture(scope="session")
def example_files(examples_dir: Path) -> list[Path]:
    return sorted(examples_dir.glob("*.mty"))
