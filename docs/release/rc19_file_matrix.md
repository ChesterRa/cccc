# RC19 File Inventory Matrix

The matrix is generated from tracked + local repo files (excluding ignored files and local virtual environments).

## Regenerate

```bash
./scripts/release/gen_rc19_file_matrix.sh
```

## Output

- CSV path: `docs/release/rc19_file_matrix.csv`
- Columns:
  - `path`
  - `tier`
  - `domain`
  - `review_mode`
  - `status`
  - `owner`
  - `notes`
- Regeneration preserves existing `status/owner/notes` by `path`; new files start as `pending/unassigned`.

Use this table as the authoritative coverage ledger for `R1` and all later audit phases.
