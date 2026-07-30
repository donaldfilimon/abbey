# Goals

## Production structure + spec
status: done
- Decomposed `main.rs` (1074→86) into session/commands/slash_dispatch/doctor/prompts
- Spec expanded: docs/architecture.md, docs/production.md, identity claims Current
- `./check.sh` production gate (fmt + clippy -D + test + file-size)
- v2.6.0 modular layout under 1k-line rule

## Hybrid routing and memory
status: done
- Personas, Max/Gemma, SQLite learn, TUI 7 tabs, OS control, parallel, inventory, build stamp

## Broader polish
status: done
