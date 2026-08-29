# Codespace validation runner

This repository uses a persistent GitHub Codespace as the self-hosted runner for the `Validate` workflow.

## First setup

Create a GitHub Codespaces development secret named `CODESPACE_RUNNER_PAT` with permission to administer repository Actions runners. The `.devcontainer/devcontainer.json` declares this as a recommended secret.

Then rebuild or recreate the Codespace from this repository. The lifecycle script installs and registers the Actions runner automatically and starts it whenever the Codespace starts.

## Validation

The workflow executes:

```bash
./scripts/validate.sh
```

The runner uses its own `_work` directory under `~/.codespace-actions-runner`, so GitHub Actions checkout does not operate on the primary VS Code workspace.

When the Codespace is stopped, the runner goes offline. Jobs targeting `self-hosted, codespace` remain queued until the Codespace is started again.
