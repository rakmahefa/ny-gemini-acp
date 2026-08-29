# Dependency policy

The workspace favors a small, explicit dependency surface.

## Rules

1. Prefer an existing workspace dependency before adding a new crate.
2. New dependencies require a concrete runtime, security, or testability justification.
3. Keep default features minimal, especially for network and async crates.
4. Keep `Cargo.lock` committed for reproducible application/workspace validation.
5. Review duplicate versions with `cargo tree -d --workspace` and investigate avoidable duplication.
6. Review enabled feature paths with `cargo tree -e features --workspace` before release.
7. Remove dead dependencies instead of retaining them for hypothetical future use.

## Audit command

```text
./scripts/dependency-audit.sh
```

The audit is informational by design: dependency duplication is a review signal, not automatically a build failure.
