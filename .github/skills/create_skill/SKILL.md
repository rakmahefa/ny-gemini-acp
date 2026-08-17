# Create Skill

## Purpose

This skill defines the standard for creating a new repository skill under `.github/skills/`.

A skill is an executable architectural instruction set for coding agents: it defines when to use a capability, what context to inspect, what invariants to preserve, how to implement changes, how to validate them, and when to stop.

The goal is to make future skills consistent, composable, reviewable, and safe to apply to `ny-gemini-acp`.

## Skill Location

Every repository skill lives at:

```text
.github/skills/<skill-name>/SKILL.md
```

Use a short, stable, lowercase kebab-case directory name.

Examples:

```text
.github/skills/architecture/SKILL.md
.github/skills/create-skill/SKILL.md
.github/skills/stream-hardening/SKILL.md
```

Do not place implementation code, generated artifacts, or unrelated documentation in a skill directory unless the skill explicitly requires them.

## Required Structure

A new skill should normally contain these sections:

1. `Purpose`
2. `When to Use`
3. `Scope`
4. `Repository Context`
5. `Invariants`
6. `Workflow`
7. `Implementation Rules`
8. `Validation`
9. `Failure / Stop Conditions`
10. `Definition of Done`

Additional sections are encouraged when they clarify a specialized domain.

## Purpose

The `Purpose` section must explain exactly what the skill governs and what outcome it is intended to produce.

Avoid vague goals such as:

```text
Help with coding.
```

Prefer:

```text
Guide incremental changes to the Gemini streaming pipeline while preserving
chunk-boundary invariance and ACP presentation integrity.
```

## When to Use

State explicit trigger conditions.

Examples:

- use for changes to a specific subsystem;
- use when a particular protocol concern is involved;
- use when a refactor crosses a defined architectural boundary.

Also state when **not** to use the skill when confusion with another skill is likely.

## Scope

Define the files, crates, layers, and responsibilities covered by the skill.

A skill must not silently claim ownership over a neighboring subsystem that belongs to another skill.

Prefer references to semantic ownership over file lists when the subsystem can move during refactoring.

## Repository Context

Describe only repository facts necessary to apply the skill correctly.

Examples:

- owning crate;
- relevant module family;
- expected public façade;
- important protocol boundary;
- authoritative existing architecture skill.

Do not copy the entire repository architecture into every specialized skill. Link the responsibility back to `architecture/SKILL.md` conceptually and document only the local rules.

## Invariants

This is the most important section.

Write behavioral guarantees, not implementation preferences.

Good invariants include:

- identical input repartitioning produces identical semantic output;
- ACP-visible output contains no internal protocol envelope;
- duplicate stream IDs are handled deterministically;
- runtime execution never depends on presentation filtering;
- lifecycle terminal events cannot be followed by success events.

An invariant should be testable or reviewable.

For each important invariant, state the failure-safe behavior.

## Workflow

Describe an ordered workflow that an agent can execute without guessing.

A strong default is:

```text
inspect -> classify -> test -> implement -> validate -> review -> document
```

The workflow should identify the smallest useful context to inspect before editing.

For repository changes, include:

1. current branch and working tree state;
2. owning crate/module;
3. relevant tests;
4. recent changes when behavior may have moved;
5. expected validation commands.

## Implementation Rules

Rules should distinguish mandatory behavior from suggestions.

Prefer explicit statements:

- MUST preserve existing public behavior unless the task intentionally changes the contract;
- MUST keep implementation state private unless another crate needs it;
- SHOULD reuse existing semantic types;
- SHOULD add regression coverage before changing ambiguous behavior;
- MUST NOT duplicate protocol constants;
- MUST NOT bypass the public façade of a subsystem without an architectural reason.

Avoid rules that merely restate coding style unless they prevent a concrete failure mode.

## Composition With Other Skills

Skills must declare their relationship to broader skills.

For a specialized skill:

```text
architecture skill = global repository rules
specialized skill = local subsystem rules
create_skill = skill-authoring rules
```

When rules conflict, the more specific skill may refine the broader rule but must not silently contradict a repository-wide invariant.

A specialized skill should say which architectural contract it inherits.

## Validation

Validation must match the skill's invariants.

Prefer a layered validation strategy:

```text
format/lint
    -> compile
    -> unit tests
    -> contract/integration tests
    -> targeted adversarial tests
```

For streaming work, include arbitrary chunk boundaries and UTF-8 cases where applicable.

For protocol work, include malformed and marker-like payloads.

For lifecycle work, include success, cancellation, and failure paths.

Do not claim a skill is validated by CI merely because the documentation file itself does not compile; validate the repository behavior that the skill governs.

## Failure / Stop Conditions

A skill must tell an agent when to stop rather than guess.

Typical stop conditions include:

- repository state contains unrelated user changes;
- the requested change conflicts with a documented protocol invariant;
- a required dependency boundary would introduce a cycle;
- semantics are ambiguous and cannot be established from tests/specification/code;
- validation reveals a regression outside the skill's safe scope.

When stopping, preserve the worktree and explain the exact blocker.

## Documentation Rules

A new skill should be concise enough to remain usable during implementation but complete enough to prevent repeated rediscovery of the same invariants.

Avoid copying long source files, generated output, or historical conversation.

Use examples only when they clarify a rule that prose cannot express precisely.

If a skill introduces a new repository-wide invariant, update the architecture skill in the same change.

If the invariant is local to the skill's subsystem, keep it local and cross-reference the architecture contract conceptually.

## Naming and Metadata

Use:

```text
.github/skills/<kebab-case-name>/SKILL.md
```

The skill title should match the directory's concept.

Avoid ambiguous names such as:

```text
misc
helper
stuff
new
```

Choose a name that describes the domain or action:

```text
architecture
create-skill
stream-hardening
mcp-protocol
acp-events
```

## Authoring Checklist

Before adding a skill, verify:

- the problem is recurring enough to justify a skill;
- an existing skill does not already own the responsibility;
- the proposed name is specific;
- the skill declares its trigger conditions;
- the skill identifies semantic ownership;
- important invariants are explicit;
- implementation steps are ordered;
- validation is concrete;
- failure/stop conditions are defined;
- architecture-wide rules are inherited rather than duplicated unnecessarily;
- no contradictory rules are introduced.

## Definition of Done

A new skill is complete when:

- the file exists at the canonical path;
- the scope and trigger are unambiguous;
- repository context is accurate;
- invariants are explicit and testable;
- workflow is actionable for an agent;
- implementation rules distinguish MUST/SHOULD behavior where useful;
- validation matches the invariant surface;
- failure conditions prevent unsafe guessing;
- the skill composes cleanly with `architecture/SKILL.md`;
- any new repository-wide invariant is documented in the architecture skill;
- the final diff contains only intentional skill changes.

## Review Checklist

Review a new skill by asking:

### Scope

- Is this really a distinct recurring capability?
- Does another skill already own part of this scope?
- Is the owning crate/module clear?

### Semantics

- Are invariants stated as behavior?
- Could two agents interpret the rules differently?
- Does the skill define safe behavior for malformed or ambiguous input?

### Workflow

- Can an agent follow the steps without guessing?
- Are tests and validation specified?
- Are stop conditions explicit?

### Architecture

- Does the skill preserve dependency direction?
- Does it accidentally create a new source of truth?
- Does it contradict or duplicate the global architecture contract?

### Maintainability

- Is the document focused enough to remain useful?
- Are examples minimal and relevant?
- Will the skill remain correct after routine refactors?

## Anti-Patterns

Do not create skills that:

- merely duplicate a README;
- restate generic Rust knowledge with no repository-specific value;
- encode a one-off bug fix instead of a reusable capability;
- prescribe exact implementation details when the invariant is what matters;
- override global architecture rules without explicit justification;
- silently depend on undocumented local state;
- provide validation claims that were not actually performed.

## Skill Evolution

Skills are versioned through normal repository history.

When a recurring implementation pattern emerges:

1. identify whether it is a new invariant or just a new example;
2. update the smallest owning skill;
3. update `architecture/SKILL.md` when the rule becomes repository-wide;
4. add or update tests if the invariant is behavioral;
5. keep unrelated skills unchanged.

The desired outcome is a small set of composable skills with clear ownership, not a large encyclopedia of overlapping instructions.
