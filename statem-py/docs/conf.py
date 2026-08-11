"""Sphinx configuration for the statem docs site.

Source lives in this directory as MyST Markdown (not reStructuredText) so the existing
hand-written `.md` pages didn't need rewriting when the project moved off MkDocs.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.abspath(".."))

project = "statem"
copyright = "2026, RahulDas-dev"  # noqa: A001
author = "RahulDas-dev"

extensions = [
    "myst_parser",
    "sphinx.ext.autodoc",
    "sphinx.ext.napoleon",
    "sphinx.ext.viewcode",
    "sphinx_autodoc_typehints",
    "sphinxcontrib.mermaid",
    "sphinx_copybutton",
]

source_suffix = {
    ".md": "markdown",
}

myst_enable_extensions = [
    "colon_fence",
    "deflist",
]
# Auto-generates GitHub-style slugs for headings (depth 1-3) so cross-page links like
# `guide.md#tracing-a-run` resolve without hand-authored anchors on every heading.
myst_heading_anchors = 3
# Lets a plain ` ```mermaid ` fence (used throughout guide.md/examples.md) render via
# sphinxcontrib-mermaid without rewriting every fence to an explicit `{mermaid}` directive.
myst_fence_as_directive = ["mermaid"]

napoleon_google_docstring = True
napoleon_numpy_docstring = False
# Renders docstring "Attributes:" sections as `:ivar:` field-list items instead of separate
# autodoc object descriptions -- avoids "duplicate object description" warnings where autodoc's
# `:members:` also introspects the same (real) class attributes on these Pydantic models.
napoleon_use_ivar = True

autodoc_member_order = "bysource"
autodoc_typehints = "description"
# `ag-ui-protocol`/`jsonpatch` are the optional `agui` extra (see stream()'s docstring) -- docs
# shouldn't require installing every optional extra just to build, so mock them out for the sole
# purpose of resolving `stream()`'s `BaseEvent` type hint.
autodoc_mock_imports = ["ag_ui", "jsonpatch"]

templates_path = ["_templates"]
exclude_patterns = ["_build", "Thumbs.db", ".DS_Store"]

html_theme = "furo"
html_title = "statem"
html_static_path = []
