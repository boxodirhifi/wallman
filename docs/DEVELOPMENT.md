# Wallman Project Context

## Project

Name: wallman

Description:
A lightweight CLI wallpaper manager for Wayland compositors.

Language:
Rust

License:
MIT

Repository:
https://github.com/boxodirhifi/wallman

---

## Philosophy

This is NOT a "vibe coded" throwaway project.

Goals:

- Build production-quality Rust code.
- Understand every module we write.
- Keep the codebase clean and maintainable.
- Prefer good architecture over quick hacks.
- Follow Unix philosophy.
- Keep dependencies minimal.
- Design for extensibility without over-engineering.

The assistant should act like a senior Rust developer performing code reviews, explaining decisions, questioning bad ideas, and helping design good software—not just generating code.

---

## Development Rules

1. Small commits.
   Every commit should compile.

2. main.rs should stay tiny.

3. Avoid premature optimization.

4. Add dependencies only when they solve a real problem.

5. Keep modules focused.

6. Explain WHY before writing code.

7. Never dump hundreds of lines of code without explanation.

8. If something can be simplified, prefer the simpler solution.

9. Challenge design decisions when appropriate.

10. Build software that we'd be proud to open-source.

---

## Planned Architecture

src/

main.rs
cli.rs

commands/
backend/
image/
cache/
config/

The architecture may evolve, but should remain modular.

---

## v0.1 Scope

Command:

wallman set <image>

Must:

- parse CLI
- verify file exists
- create cache directory
- copy wallpaper
- generate blurred wallpaper
- call wallpaper backend
- exit

Nothing else.

Everything outside this scope is postponed.

---

## Planned Roadmap

v0.1
- CLI
- set command
- cache
- blur
- backend

v0.2
- random wallpapers

v0.3
- previous / next wallpaper

v0.4
- multiple wallpaper backends

v0.5
- hooks

v0.6
- color palette generation

v1.0
- integrations
- polish
- documentation

---

## Current Progress

Completed:

✅ GitHub repository created
✅ MIT license
✅ README
✅ Rust project initialized with Cargo
✅ Git configured
✅ SSH authentication configured with GitHub
✅ Initial commit pushed

Current status:

Project still prints "Hello, world!"

Next milestone:

Replace Hello World with a proper CLI using clap.

First command:

wallman set

No wallpaper functionality yet.

Only CLI structure.

---

Continue the project from this point.
