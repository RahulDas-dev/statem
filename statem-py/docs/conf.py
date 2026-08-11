"""Sphinx configuration for the statem docs site.

Source lives in this directory as MyST Markdown (not reStructuredText) so the existing
hand-written `.md` pages didn't need rewriting when the project moved off MkDocs.

Theme/layout (Alabaster, the `github_banner` ribbon, the custom sidebar, `FlaskyStyle`
Pygments colors) deliberately mirrors https://requests.readthedocs.io/ -- see
`docs/_templates/sidebar.html` and `docs/_pygments/flasky_style.py` (the latter vendored
verbatim from the Requests project, MIT-licensed) for the pieces that aren't just config.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.abspath(".."))
sys.path.insert(0, os.path.abspath("_pygments"))

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
exclude_patterns = ["_build", "_pygments", "Thumbs.db", ".DS_Store"]

# Pygments code-highlighting palette -- see docs/_pygments/flasky_style.py.
pygments_style = "flasky_style.FlaskyStyle"

# Alabaster ships with Sphinx itself (no extra dependency), and is what Requests' docs
# actually use -- see the module docstring above.
html_theme = "alabaster"
html_theme_options = {
    "show_powered_by": False,
    "github_user": "RahulDas-dev",
    "github_repo": "statem",
    "github_banner": True,
    "show_related": False,
    "note_bg": "#FFF59C",
}
html_title = "statem"
html_static_path = ["_static"]
html_logo = "_static/statem-logo.svg"
html_favicon = "_static/statem-favicon.png"
html_show_sourcelink = False
html_show_sphinx = False

# "sidebar.html" (in docs/_templates/) replaces Alabaster's default "about.html" -- same
# structure Requests uses: logo/star-button/blurb up top, curated links, then localtoc +
# relations + searchbox for in-page navigation. ("sourcelink.html" is omitted since
# html_show_sourcelink=False already hides it -- Requests keeps it listed but it's a no-op.)
html_sidebars = {
    "index": ["sidebar.html", "searchbox.html"],
    "**": ["sidebar.html", "localtoc.html", "relations.html", "searchbox.html"],
}
