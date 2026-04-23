# IonVM Git Workflow (AI-Optimized)

> **LLM Usage:** Standard for commits and PRs. Follow this to maintain repository health.

## 🌿 Branching
- `main`: Stable release branch.
- `feat/`, `fix/`, `docs/`: Development branches.

## 💬 Commit Messages
Format: `<type>(<scope>): <subject>`
- **Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.
- **Scope examples:** `vm`, `ffi`, `py`, `mesh`.
- **Constraint:** One logical change per commit.

## 🔄 PR Process
1. **Research:** Define impact on `BYTECODE.md` or `ARCHITECTURE.md`.
2. **Implement:** Code + Tests.
3. **Validate:** `cargo test` and `pytest` must pass.
4. **Docs:** Update corresponding specification if ISA or format changes.
