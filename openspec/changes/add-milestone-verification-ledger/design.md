## Context

See `proposal.md` - Why. Relevant constraints already fixed by prior
decisions in this project (not re-litigated here):

- The development plan (`docs/`) is deliberately kept out of git for now
  (solo-owned project) — see `.gitignore` and the project's revision
  history. This change should stay consistent with that boundary rather
  than reopen it.
- Enforcement is convention-only for now, not a blocking hook (explicit
  non-goal below, per the proposal).
- Each phase chapter (`docs/mp3-encoder/0X-phaseN-*.md`) already states
  its own Definition of Done as a prose checklist. This mechanism does
  not replace that prose — it adds a machine-executable, evidence-backed
  layer underneath it.

## Goals / Non-Goals

**Goals:**
- A ledger entry can only be created by actually executing the commands
  it claims passed — never by an LLM writing prose into the ledger.
- Every entry is anchored to an exact, retrievable commit — a PASS that
  doesn't correspond to a real, inspectable code state is worthless.
- A cold-start session can learn current, evidence-backed status by
  reading one small generated file — no re-verification, no reading
  prior session transcripts, no reading source.
- Both successes and failures are recorded — a failed run is evidence
  too (regression visibility), not something to discard.

**Non-Goals (this change):**
- No harness-enforced blocking mechanism (e.g. a Claude Code `Stop` hook
  refusing to end a session without a fresh PASS). Named in the proposal
  as a candidate for later if convention proves insufficient — not built
  here.
- No CI integration. The ledger works the same whether run by a human,
  an LLM in an interactive session, or (later) a CI job — but wiring
  actual CI is out of scope.
- No cryptographic signing / tamper-proof chain-of-custody beyond a
  content hash of raw output. Proportionate to a solo-owned project; not
  designed to resist a malicious actor, only to make silent
  self-attestation impossible under normal LLM-agent behavior.
- No coverage metrics or quality scoring beyond pass/fail of the
  commands each milestone's DoD already specifies.

## Decisions

### 1. Everything lives under `docs/mp3-encoder/verification/`, not `crates/` or a top-level `scripts/`

The script, its manifest, the ledger, and the generated summary are all
colocated under the existing `docs/` privacy boundary
(`docs/mp3-encoder/verification/{verify.sh,manifest.yaml,ledger.ndjson,STATUS.md,runs/}`).

**Alternative considered**: a Rust `xtask` workspace member (the
idiomatic `cargo xtask` pattern), invoked as `cargo run -p xtask --
verify M3`. Rejected for this change: it would need to be a tracked
workspace member (Cargo requires workspace members to exist for `cargo
build --workspace` to succeed at all), which would force part of this
system to be versioned while its sibling — the manifest, which encodes
milestone-specific knowledge inseparable from the private plan — stays
untracked. That split is more confusing than a plain script, and the
proposal already commits to "no impact on `crates/`." A shell script
avoids this entirely and is proportionate to what it actually does:
shell out to `cargo`/`ffmpeg` and append a line of JSON.

**Alternative considered**: script under a tracked top-level `scripts/`,
only the ledger/manifest under `docs/`. Rejected: the manifest is
essentially a machine-readable mirror of the milestone DoDs defined in
the (private) plan, and splitting "the tool" from "the data it's
inseparable from" across the tracked/untracked boundary invites drift
with no real benefit while the project is solo and the plan stays
private.

### 2. Bash script, not Rust

Follows from Decision 1 (no new workspace member). `jq` is used to build
each NDJSON record when available; the script checks for it at startup
and falls back to a small manual escaper for the fixed, known field set
it emits (no free-form text is embedded raw — command output is only
ever hashed, never inlined).

### 3. Manifest format: YAML, one entry per milestone

Mirrors each phase chapter's DoD as an executable list of named checks:

```yaml
milestones:
  M1:
    checks:
      - name: test-m1
        cmd: cargo test -p mp3-core --test m1_framing
      - name: clippy
        cmd: cargo clippy --workspace --all-targets -- -D warnings
      - name: fmt
        cmd: cargo fmt --check
  M8:
    checks:
      - name: test-m8
        cmd: cargo test -p mp3-core --test m8_bitstream
      - name: clippy
        cmd: cargo clippy --workspace --all-targets -- -D warnings
      - name: fmt
        cmd: cargo fmt --check
      - name: external-decode-ffmpeg
        cmd: docs/mp3-encoder/verification/checks/ffmpeg_decode.sh
        external: true   # per 13-testing-and-validation.md: skip (not
                          # fail) when the tool isn't installed locally
```

The full M0-M9 manifest content is implementation detail for `tasks.md`,
not this document. `external: true` checks that are unavailable record
as `SKIPPED`, not `PASS` — see Decision 6.

### 4. Ledger schema: NDJSON, one record per run, both PASS and FAIL recorded

```json
{"schema_version":1,"milestone":"M3","timestamp":"2026-08-20T18:04:11Z","commit":"a1b2c3d4e5f6...","result":"PASS","summary":"3/3 checks passed","checks":[{"name":"test-m3","result":"PASS"},{"name":"clippy","result":"PASS"},{"name":"fmt","result":"PASS"}],"output_sha256":"...","run_log":"runs/2026-08-20T180411Z-M3.log"}
```

`schema_version` allows the format to evolve without breaking old
records. `run_log` points at the full raw output, saved alongside (see
Decision 5) — the hash alone proves nothing changed after the fact, but
isn't useful for debugging a failure on its own.

### 5. Full raw output is saved to `runs/<timestamp>-<milestone>.log`, hashed into the ledger record

A hash-only record is enough to *detect* tampering but not to *debug* a
failure. Saving the full log costs negligible disk space and is fully
reproducible (re-running regenerates it), so it isn't precious — treat
`runs/` as prunable working state, unlike the ledger itself.

### 6. Hard refusal on a dirty working tree

The script runs `git status --porcelain` before executing any checks. If
the tree isn't clean, it refuses to write a ledger entry at all (prints
what's dirty and exits non-zero). Anchoring a PASS to a commit hash is
the entire point of this mechanism (proposal - Why); a PASS recorded
against uncommitted, unreproducible state would defeat it silently.

### 7. `result` has three values: `PASS`, `FAIL`, `PARTIAL`

`PARTIAL` covers the case where every *runnable* check passed but a
milestone-required `external: true` check was `SKIPPED` (tool not
installed) — this must not be indistinguishable from a genuine `PASS`,
since from M8 onward the external-decoder check is not optional per that
milestone's actual Definition of Done.

### 8. Two files, two purposes — not one file trying to serve both

`docs/mp3-encoder/14-roadmap-and-milestones.md`'s existing status table
stays as-is in spirit: the human-authored plan/checklist expressing
*intent* (what should be true once a milestone is implemented).
`docs/mp3-encoder/verification/STATUS.md` is fully generated from the
ledger (never hand-edited — its header says so) and expresses *fact*:
what has actually been verified, against which commit, when. The
roadmap gets one addition: each milestone row links to `STATUS.md`
instead of carrying hand-written pass/fail prose. Trying to make one
file both the plan and the evidence log invites exactly the drift this
change exists to eliminate.

## Risks / Trade-offs

- **[Risk]** Convention-only enforcement means an LLM could still
  hand-edit the ledger or skip the script entirely. → **Mitigation**:
  explicit, repeated instructions in the roadmap doc and `CLAUDE.md`;
  `STATUS.md`'s "machine-generated, do not edit" header makes a
  hand-edit visually anomalous on review; a harness-enforced gate is the
  named next step if this proves insufficient (non-goal above, not
  forgotten).
- **[Risk]** Bash + optional `jq` dependency. → **Mitigation**: startup
  check with a manual-escaping fallback for the fixed field set emitted.
- **[Risk]** Manifest and phase-chapter DoD prose can drift (edit one,
  forget the other). → **Mitigation**: documented coupling in both
  files' headers now; a cross-check lint is a reasonable future addition,
  not required for this change to be useful.
- **[Risk]** `runs/` log directory grows unbounded across many sessions.
  → **Mitigation**: fully reproducible and prunable by design; only the
  NDJSON ledger needs to persist indefinitely.

## Migration Plan

- Net-new — no existing ledger to migrate.
- First real run backfills history: M0 was previously verified manually
  (current roadmap row). Re-running the M0 manifest once through the
  real script produces the first trustworthy ledger entry, replacing the
  hand-written claim rather than leaving it as the only record.
- Rollback: delete `docs/mp3-encoder/verification/`. Nothing outside
  that directory structurally depends on it — the roadmap table can
  revert to hand-written prose if this mechanism is ever abandoned.

## Open Questions

- Retention policy for `runs/*.log` (keep forever vs. prune after N runs
  per milestone) — low-stakes, fully reproducible either way, safe to
  decide during implementation without affecting the ledger schema or
  approach above.
