## ADDED Requirements

### Requirement: Goal → Subgoal → Task hierarchy

The goal system SHALL model subgoals (replacing milestones), giving goals a `Goal → Subgoal → Task` decomposition with progress rollup. [P2]

#### Scenario: A goal decomposes into subgoals

- **WHEN** a subgoal is added to a goal
- **THEN** the goal lists the subgoal and `Goal::progress` reflects completed-vs-total subgoals

#### Scenario: Subgoal completion rolls up

- **WHEN** all of a goal's subgoals are completed
- **THEN** the goal's progress reports full completion

### Requirement: Persistent goal storage

The `TaskBoard` (goals, subgoals, tasks) SHALL persist to `~/.openassistant/goals.json` and reload on startup, written atomically. [P2]

#### Scenario: Goals survive a restart

- **WHEN** a goal with subgoals is created and the process exits
- **THEN** a new process loading the store sees the goal and its subgoals

#### Scenario: Writes are atomic

- **WHEN** the store is saved
- **THEN** it is written to a temporary file in the same directory and atomically persisted over the target, so a crash mid-write cannot truncate `goals.json`

### Requirement: Real goal deliberation

`handle_goal_deliberate` SHALL produce real per-role analysis by calling the LLM, rather than emitting placeholder "In a full implementation…" text. [P2]

#### Scenario: Each deliberation role calls the LLM

- **WHEN** a goal deliberation runs
- **THEN** each role (Judge, Devil's Advocate, Researcher, Executor, Synthesizer) issues an LLM call with its role prompt and the accumulated context
- **AND** the contributions persisted to the goal store contain real model output

### Requirement: Goal CLI and tool surface

The agent SHALL expose `goal_create`/`goal_subgoal`/`goal_list`/`goal_task` tools, and a `goals` CLI command SHALL create and list goals and subgoals. [P2]

#### Scenario: Create and list goals from the CLI

- **WHEN** the user runs `goals --action create --title "Ship v1"` then `goals --action list`
- **THEN** the goal is persisted and appears in the list with its subgoal progress
