# statem

A minimal state machine engine, built as two independent, idiomatic sibling libraries:
[`statem-py`](statem-py/) (Python, published to PyPI as `statem`) and
[`statem-rs`](statem-rs/) (Rust crate `statem-rs`). Same design philosophy, two implementations
— see each directory's own docs for language-specific usage.

## Why this shape

- **Config is data, not code.** The state graph (`on` / `always` / `entry` / `exit` /
  `error_state`) is a plain, JSON/YAML-friendly structure — author it by hand, generate it, or
  load it from a file, database, or LLM.
- **Guards and actions are ordinary functions**, registered by name, independent of the graph.
  No subclassing, no framework lifecycle to inherit into.
- **Fails fast.** Bad transition targets and unregistered guard/action names are caught at
  construction time, not three hops into a live run.
- **Every run is traceable.** Each guard/action that fires is recorded in order, so you can see
  exactly why the machine ended up where it did.
- **One philosophy, two languages, on purpose.** `statem-py` and `statem-rs` aren't a port of one
  into the other — each is written the way its language naturally does this: Pydantic validation
  and duck-typed sessions in Python; the borrow checker, `Display`, and typed errors in Rust.

## Packages

| Package | Language | Docs |
|---|---|---|
| [`statem-py`](statem-py/) | Python (PyPI: `statem`) | [statem-py/README.md](statem-py/README.md) |
| [`statem-rs`](statem-rs/) | Rust (crate: `statem-rs`) | [statem-rs/Cargo.toml](statem-rs/Cargo.toml) |

## License

MIT — see [LICENSE](LICENSE).
