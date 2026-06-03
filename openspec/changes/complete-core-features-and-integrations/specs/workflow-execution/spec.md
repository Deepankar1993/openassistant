## ADDED Requirements

### Requirement: Real workflow step execution

`WorkflowEngine` SHALL execute each step by making a real LLM call (via the shared `call_llm_raw`) instead of returning a fabricated "Step completed" string. Steps without satisfied dependencies SHALL receive the outputs of their dependency steps as context. [P1]

#### Scenario: A step calls the LLM

- **WHEN** a workflow is executed via an engine constructed with `new_with_config`
- **THEN** each ready step issues a `call_llm_raw` request whose prompt includes the step description and the outputs of its `depends_on` steps
- **AND** the step's `StepResult` records the LLM output

#### Scenario: Dependency outputs flow downstream

- **WHEN** step B `depends_on` step A
- **THEN** B's prompt contains A's produced output

#### Scenario: The stub path is preserved

- **WHEN** an engine is constructed via `WorkflowEngine::new()`/`default()` without config
- **THEN** the previous (non-LLM) behavior remains available so existing callers do not break

### Requirement: Persistent workflow runs

Workflow runs and step results SHALL be persisted to a dedicated `~/.openassistant/workflows.db` (WAL + foreign keys), so `list_runs` returns real history rather than an empty vector. [P1]

#### Scenario: Runs are queryable after completion

- **WHEN** a workflow finishes
- **THEN** its run row and step-result rows are written to `workflows.db`
- **AND** `list_runs()` returns the persisted runs and `get_run_status(run_id)` returns its status

### Requirement: Honest failure and built-ins

The engine SHALL distinguish a failed dependency from a genuine deadlock, and SHALL NOT ship a built-in workflow that cannot actually run. [P1]

#### Scenario: A failed dependency skips dependents

- **WHEN** a step errors
- **THEN** a `Failed` `StepResult` is recorded and its dependents are reported as "skipped: dependency failed" rather than triggering a "Workflow deadlock" error

#### Scenario: Built-in workflow runs without tools

- **WHEN** the shipped built-in workflow is executed
- **THEN** it is a tool-free `analyze → critique → summarize` chain that produces real LLM output, not a tool-dependent workflow that would hallucinate
