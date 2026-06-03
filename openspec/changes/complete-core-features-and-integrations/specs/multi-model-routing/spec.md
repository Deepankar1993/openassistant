## ADDED Requirements

### Requirement: Multi-provider configuration

`Config` SHALL support a list of named providers and a per-modality routing map, in addition to the existing single `model` block, without breaking existing config files. All new structures SHALL default to empty. [P1]

#### Scenario: Legacy config loads unchanged

- **WHEN** a `config.yaml` containing only the legacy `[model]` block (no `providers`/`routing`) is loaded
- **THEN** it deserializes successfully with `providers` empty and `routing` all-empty
- **AND** it re-serializes and round-trips without losing the `model` fields

#### Scenario: Providers and routes are configured by editing the file

- **WHEN** the user adds `providers:` entries and `routing:` modality routes to `config.yaml`
- **THEN** they load into `Config.providers` and `Config.routing`
- **AND** `config::set` does NOT accept `providers.*`/`routing.*`/`model.api_base` keys (security boundary preserved)

### Requirement: Modality-based provider resolution

A `resolve_provider(config, modality)` function SHALL return the `(api_base, api_key, model)` to use for a given modality (`text`, `vision`, `image_gen`, `video`), falling through to the legacy `model` block when the route is empty. Routing SHALL be strictly opt-in. [P1]

#### Scenario: Empty routing reproduces legacy behavior

- **WHEN** `routing.text` is empty
- **THEN** `resolve_provider(config, "text")` returns the legacy `(model.api_base, model.api_key, model.model)`
- **AND** the agent's existing `Agent::new(model)` behavior is unchanged

#### Scenario: A configured route selects its provider

- **WHEN** `routing.text` names a provider that exists in `providers[]`
- **THEN** `resolve_provider(config, "text")` returns that provider's `api_base`/`api_key` and the route's `model`

#### Scenario: Text dispatch uses the resolved provider

- **WHEN** the agent makes its LLM call
- **THEN** it sends the request to the `api_base`/`api_key`/`model` resolved for the `text` modality
- **AND** `vision`/`image_gen`/`video` routes are parsed and stored but not dispatched in this change
