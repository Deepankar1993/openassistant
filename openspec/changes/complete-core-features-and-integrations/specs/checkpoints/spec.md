## ADDED Requirements

### Requirement: Persistent checkpoint storage

`CheckpointStore` SHALL persist checkpoints in a dedicated SQLite database (`~/.openassistant/checkpoints.db`) so that checkpoints survive process restarts, replacing the in-memory `Vec`. Each connection SHALL enable `PRAGMA journal_mode=WAL` and `PRAGMA foreign_keys=ON`, and file rows SHALL cascade-delete with their checkpoint. [P0]

#### Scenario: Checkpoints survive a restart

- **WHEN** a checkpoint is created in one process and a new process opens the same `checkpoints.db`
- **THEN** the checkpoint and its file snapshots are listable and restorable in the new process

#### Scenario: Creating a checkpoint snapshots files

- **WHEN** `create_checkpoint(session, description, workspace, files)` is called
- **THEN** a `checkpoints` row and one `checkpoint_files` row per readable UTF-8 file are inserted in a single transaction, each file row storing its content and SHA-256 hash
- **AND** a checkpoint id is returned

#### Scenario: Listing returns lightweight metadata

- **WHEN** `list_checkpoints(session)` is called
- **THEN** it returns metadata (id, session, timestamp, description, file count) without loading file bodies
- **AND** file bodies are obtained separately via `load_checkpoint(id)`

#### Scenario: Pruning keeps only the most recent per session

- **WHEN** the number of checkpoints for a session exceeds the maximum
- **THEN** the oldest are deleted and their file rows cascade-delete

### Requirement: Safe checkpoint restore

`restore_checkpoint` SHALL NOT silently overwrite a file whose current contents differ from the snapshot, protecting unsaved work. [P0]

#### Scenario: Unchanged files are restored

- **WHEN** a file's current content hash matches the snapshot hash (or the file is absent)
- **THEN** the file is written from the snapshot and reported as restored

#### Scenario: Changed files are skipped unless forced

- **WHEN** a target file's current content hash differs from the snapshot hash and `force` is false
- **THEN** the file is skipped with a warning and reported as skipped
- **WHEN** the same restore is run with `force` true
- **THEN** the file is overwritten from the snapshot

### Requirement: Checkpoint CLI surface

The `checkpoint` command SHALL expose real `list`, `create`, and `restore` actions. [P0]

#### Scenario: Create from the CLI

- **WHEN** the user runs `checkpoint --action create --workspace <dir> --files a.rs,b.rs --description "before refactor"`
- **THEN** a checkpoint is created and its id is printed
- **AND** when `--session` is omitted it defaults to a stable hash of the absolute workspace path

#### Scenario: Restore from the CLI

- **WHEN** the user runs `checkpoint --action restore --id <checkpoint-id> --workspace <dir>`
- **THEN** the checkpoint's files are restored subject to the dirty-check, and `--force` bypasses the dirty-check
