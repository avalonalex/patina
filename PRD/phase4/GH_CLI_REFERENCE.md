# GitHub CLI (gh) Quick Reference

Quick reference for using GitHub's `gh` command line tool with Patina.

## Installation

```bash
# macOS
brew install gh

# Ubuntu/Debian
sudo apt install gh

# Other platforms: https://github.com/cli/cli#installation
```

## Authentication

```bash
# Login to GitHub
gh auth login

# Check authentication status
gh auth status

# Logout
gh auth logout
```

## Repository Operations

```bash
# Clone a repository
gh repo clone owner/repo

# View repository in browser
gh repo view --web

# Create a new repository
gh repo create patina --public --source=. --remote=origin

# Fork a repository
gh repo fork owner/repo
```

## Pull Requests

```bash
# Create a PR from current branch
gh pr create

# Create PR with title and body
gh pr create --title "Add feature X" --body "Description here"

# Create draft PR
gh pr create --draft

# List PRs
gh pr list

# View a specific PR
gh pr view 123
gh pr view 123 --web  # Open in browser

# Check out a PR locally
gh pr checkout 123

# Review a PR
gh pr review 123 --approve
gh pr review 123 --request-changes --body "Comments here"
gh pr review 123 --comment --body "Just a comment"

# Merge a PR
gh pr merge 123
gh pr merge 123 --squash
gh pr merge 123 --rebase

# Close a PR
gh pr close 123

# Reopen a PR
gh pr reopen 123

# Check PR status/checks
gh pr checks 123
```

## Issues

```bash
# Create an issue
gh issue create
gh issue create --title "Bug: REPL crash" --body "Details here"

# List issues
gh issue list
gh issue list --label bug
gh issue list --state all

# View an issue
gh issue view 42
gh issue view 42 --web

# Close an issue
gh issue close 42

# Reopen an issue
gh issue reopen 42

# Comment on an issue
gh issue comment 42 --body "Update here"
```

## Workflow/Actions

```bash
# List workflows
gh workflow list

# View workflow runs
gh run list

# View a specific run
gh run view 123456

# Watch a run in progress
gh run watch

# Rerun a failed workflow
gh run rerun 123456

# View workflow logs
gh run view 123456 --log
```

## Releases

```bash
# Create a release
gh release create v1.0.0 --title "Version 1.0.0" --notes "Release notes"

# List releases
gh release list

# View a release
gh release view v1.0.0

# Download release assets
gh release download v1.0.0

# Upload assets to a release
gh release upload v1.0.0 patina-linux-x64.tar.gz
```

## Gists

```bash
# Create a gist from a file
gh gist create file.scm

# Create a public gist
gh gist create file.scm --public

# List your gists
gh gist list

# View a gist
gh gist view <gist-id>
```

## Useful Patterns for Patina Development

### Quick PR workflow
```bash
# Create feature branch
git checkout -b feature/lambda-closures

# Make changes, commit
git add .
git commit -m "Implement proper closures for lambda"

# Push and create PR in one go
git push -u origin feature/lambda-closures
gh pr create --fill  # Uses commit messages for PR description

# View PR checks
gh pr checks

# Merge when ready
gh pr merge --squash
```

### View CI status
```bash
# Watch current branch's CI
gh run watch

# List recent runs
gh run list --limit 5

# View logs for failed run
gh run view <run-id> --log-failed
```

### Working with issues
```bash
# Create issue from template
gh issue create --web

# Link PR to issue
gh pr create --body "Closes #42"

# List open bugs
gh issue list --label bug --state open
```

### Check PR before merging
```bash
# View PR details
gh pr view 123

# Check CI status
gh pr checks 123

# View diff
gh pr diff 123

# View changed files
gh pr view 123 --json files --jq '.files[].path'
```

## Configuration

```bash
# Set default editor
gh config set editor vim

# Set default git protocol
gh config set git_protocol ssh

# View all config
gh config list

# Set default repo (run in repo directory)
gh repo set-default
```

## Advanced Features

### JSON output for scripting
```bash
# Get PR data as JSON
gh pr view 123 --json number,title,state,author

# Process with jq
gh pr list --json number,title,author | jq '.[] | select(.author.login == "username")'

# Get issue labels
gh issue view 42 --json labels --jq '.labels[].name'
```

### Aliases
```bash
# Create custom alias
gh alias set prs 'pr list --author @me'
gh alias set bugs 'issue list --label bug'

# List aliases
gh alias list
```

## Help

```bash
# General help
gh help

# Command-specific help
gh pr --help
gh issue create --help

# Manual pages
man gh
man gh-pr-create
```

## Common Workflows for Patina

### After implementing a feature
```bash
# Push branch
git push -u origin feature-name

# Create PR
gh pr create --title "Implement tail call optimization" \
  --body "Adds proper tail call optimization for R7RS compliance. Closes #15"

# Monitor CI
gh run watch
```

### Reviewing test results in CI
```bash
# List recent runs
gh run list --branch main --limit 5

# View specific run
gh run view <run-id>

# Download test artifacts (if configured)
gh run download <run-id>
```

### Release workflow
```bash
# Tag version
git tag v0.1.0
git push origin v0.1.0

# Create release with binaries
gh release create v0.1.0 \
  --title "v0.1.0 - Basic R7RS features" \
  --notes-file CHANGELOG.md \
  target/release/patina
```
