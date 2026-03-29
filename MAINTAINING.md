# Maintaining ultrajpeg

## Release Tag

Create and push the release tag that matches `Cargo.toml`:

```bash
cargo run -p xtask -- release
```

The command:

- reads `package.version` from the root `Cargo.toml`
- refuses to run if the repository has any tracked or untracked changes
- creates the `v{package.version}` git tag
- pushes that tag to the `origin` remote

If the push fails after the local tag is created, the command prints the manual retry command to use.
