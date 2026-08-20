## 1. Scaffold the verification directory

- [x] 1.1 Create `docs/mp3-encoder/verification/` with a `runs/` subdirectory and verify the directory structure exists
- [x] 1.2 Write `manifest.yaml` with M0's checks (build, wasm build, test, clippy, fmt — mirroring `03-project-setup.md`'s Definition of Done) and verify it parses as valid YAML

## 2. Core script: run a milestone's checks and record the result

- [ ] 2.1 Implement `verify.sh`'s dirty-working-tree guard (`git status --porcelain`) and verify it refuses to proceed (non-zero exit, no ledger write) on a dirty tree
- [ ] 2.2 Implement manifest lookup for a given milestone id (`yq` if available, minimal fallback parser otherwise) and verify it lists the correct checks for M0
- [ ] 2.3 Implement per-check execution capturing exit code and output, and verify running against M0 produces the correct per-check PASS/FAIL results
- [ ] 2.4 Implement `sha256` hashing of full combined output plus saving it to `runs/<timestamp>-<milestone>.log`, and verify the saved log's hash matches the recorded `output_sha256`
- [ ] 2.5 Implement NDJSON record append to `ledger.ndjson` (`jq` if available, fallback escaper for the fixed field set) with the `design.md` §4 schema (`schema_version`, `milestone`, `timestamp`, `commit`, `result`, `summary`, `checks`, `output_sha256`, `run_log`), and verify the appended line is valid JSON matching that schema
- [ ] 2.6 Implement `external: true` check handling — records `SKIPPED` when the check's tool/command isn't available, and the run's overall `result` becomes `PARTIAL` (never `PASS`) when a milestone-required external check was skipped; verify with a synthetic external check

## 3. Human-readable summary

- [ ] 3.1 Implement `verify.sh summary`, regenerating `STATUS.md` from the ledger's latest record per milestone, with a "machine-generated, do not edit" header, and verify it reflects the current ledger contents after a run

## 4. Wire into the project

- [ ] 4.1 Update `docs/mp3-encoder/14-roadmap-and-milestones.md`'s status table to link each milestone row to `verification/STATUS.md` instead of carrying hand-written pass/fail prose
- [ ] 4.2 Add explicit instructions to the roadmap doc (and/or `CLAUDE.md`) mandating that closing a milestone requires running `verify.sh run <milestone>` — never hand-editing the ledger or `STATUS.md`
- [ ] 4.3 Run `verify.sh run M0` for real against the current commit and verify a genuine `PASS` entry is recorded, backfilling M0's previously-manual verification with real evidence
