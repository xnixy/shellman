# Contributing

This is a personal project, but bug reports and ideas are welcome. Open an issue and tell me:

- your Windows version (Settings > System > About),
- whether shellman was your shell or running beside Explorer,
- what you did, what you expected, and what happened instead.

If something crashed or misbehaved, `%LOCALAPPDATA%\shellman\session.log` usually says why.

Pull requests are fine too, but open an issue first for anything bigger than a small fix, so we
can agree on it before you spend time on it.

A few things I stick to (more in [docs/PLAN.md](docs/PLAN.md)):

- Build and test next to Explorer. Never make your build the real shell to try something.
- Nothing slow on the UI thread. No per-frame work that grows with the number of items.
- Small files, one job each.
- Run `cargo test` for what you touched.
