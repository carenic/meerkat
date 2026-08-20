# 004 — CLI One-Liners (Shell)

Common `rkat` workflows from the command line, with no application code.

## Concepts
- `rkat run` — single-turn agent execution
- `rkat run --resume last` — multi-turn session resumption
- `rkat session list` - inspect sessions created by earlier commands
- `--isolated` / `--realm` - realm selection
- `--verbose` / `--stream` - output modes
- `rkat config` - runtime configuration

## Prerequisites
```bash
export ANTHROPIC_API_KEY=sk-...
./scripts/repo-cargo build -p rkat --bin rkat
```

## Run
```bash
./examples/004-cli-one-liners-sh/examples.sh
```
