// Developer tooling, never shipped: it runs git, cargo and other tools itself.
#![allow(clippy::disallowed_methods)]

fn main() -> anyhow::Result<()> {
    xtask::run()
}
