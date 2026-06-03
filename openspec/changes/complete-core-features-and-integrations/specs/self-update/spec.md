## ADDED Requirements

### Requirement: Source-based `update` command

The `update` CLI command SHALL perform a real git-based source update via the existing `SelfUpdater`, replacing the placeholder that only printed instructions. It SHALL locate the source checkout, report whether updates are pending, and — on confirmation — pull and rebuild. It operates on source checkouts only; binary/desktop self-update is out of scope. [P0]

#### Scenario: Pending updates are detected

- **WHEN** the user runs `openassistant update --check`
- **THEN** the command resolves the source directory, runs `git fetch`, and lists pending commits (`git log HEAD..origin/HEAD --oneline`)
- **AND** it exits 0 without modifying anything

#### Scenario: Already up to date

- **WHEN** `update` runs and `git log HEAD..origin/HEAD` is empty
- **THEN** the command prints that the install is already up to date and exits 0 without prompting

#### Scenario: Apply with confirmation

- **WHEN** the user runs `openassistant update` and there are pending commits
- **THEN** the command prints the pending commits and prompts for confirmation, unless `--yes` is passed
- **AND** on confirmation it runs `git pull --rebase` then `cargo build --release`
- **AND** it reports that the new binary is in `target/release/`, that the running process keeps the old version until restart, and that `cargo install --path .` is needed if installed via cargo

#### Scenario: Dirty working tree is refused

- **WHEN** `update` is asked to apply and `git status --porcelain` is non-empty
- **THEN** the command aborts with a message to commit or stash changes first, and does NOT run `git pull`

#### Scenario: Source directory cannot be located

- **WHEN** `update` runs and neither `OPENASSISTANT_SRC` is set nor a parent of the executable contains a `Cargo.toml` with `name = "open-assistant"`
- **THEN** the command errors with a message naming the `OPENASSISTANT_SRC` environment variable

### Requirement: Truthful update plumbing

`SelfUpdater` SHALL not report success when its subprocess actually failed, and the dead crates.io path SHALL be made explicit. [P0]

#### Scenario: A failed build is not reported as success

- **WHEN** the spawned `cargo build` task panics or the process fails to launch
- **THEN** `update_from_source` returns an error (the join/IO error is propagated), not `Ok`

#### Scenario: crates.io check is not silently misleading

- **WHEN** `check_update()` is invoked
- **THEN** it returns an explanatory error noting the crate is not published, rather than always returning `Ok(None)` as if no update exists
