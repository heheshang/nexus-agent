# AI-SDLC Review Policy

Calibration context for the automated review agents. Apply the principles
below before analyzing a PR. Edit freely: this file is yours.

## Golden Rule

**When in doubt, approve with a suggestion; do not request changes.**

## Review Principles

1. **Evidence-first**: trace the code path or do not flag it.
2. **Deterministic-first**: defer to CI for lint, types and coverage.
3. **Trust boundaries**: flag only at real untrusted boundaries.
4. **Severity honesty**: no failure scenario means not critical or major.
5. **Signal over noise**: one good finding beats ten weak ones.
6. **Scope discipline**: do not flag deferred work or unchanged code.
