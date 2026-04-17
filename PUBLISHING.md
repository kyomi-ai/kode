# Publishing kode to crates.io

Every crate in this workspace (except `demo`) is published to crates.io by the
[`publish-crates.yml`](.github/workflows/publish-crates.yml) GitHub Actions
workflow. Tag a version, push the tag, and the workflow publishes everything
in the correct order.

## Published crates

Published in this order (topological — later crates depend on earlier ones):

| Tier | Crate | Depends on |
|------|-------|------------|
| 1 | `kode-core` | — |
| 1 | `kode-doc` | — |
| 2 | `kode-markdown` | `kode-core` |
| 3 | `kode-leptos` | `kode-core`, `kode-doc`, `kode-markdown` |

`kode-demo` is `publish = false` and stays in-repo.

## One-time bootstrap (first publish ever)

crates.io's [Trusted Publishing](https://blog.rust-lang.org/2025/06/20/OIDC-trusted-publishing-cratesio/)
can only be configured on a crate that already exists on crates.io. So the
very first publish uses a legacy API token; subsequent publishes use OIDC.

### 1. Create a crates.io API token

1. Sign in to https://crates.io/ with your GitHub account.
2. https://crates.io/settings/tokens → **New Token**.
3. Name: `kode-bootstrap`. Scope: **publish-new** + **publish-update**.
4. Copy the token.

### 2. Add it as a GitHub Actions secret

1. https://github.com/kyomi-ai/kode/settings/secrets/actions → **New repository secret**.
2. Name: `CARGO_REGISTRY_TOKEN`. Value: paste the token. Save.

### 3. Tag and push

```bash
git tag v0.1.0
git push origin v0.1.0
```

The [Publish to crates.io](https://github.com/kyomi-ai/kode/actions/workflows/publish-crates.yml)
workflow will run. All four crates should land on crates.io within a few
minutes.

### 4. Configure Trusted Publishing per crate

Once each crate is live on crates.io:

1. Visit `https://crates.io/crates/<crate>/settings` (e.g.
   https://crates.io/crates/kode-core/settings).
2. Under **Trusted Publishing**, add a GitHub configuration:
   - **Repository owner**: `kyomi-ai`
   - **Repository name**: `kode`
   - **Workflow filename**: `publish-crates.yml`
   - **Environment**: leave blank
3. Save. Repeat for `kode-doc`, `kode-markdown`, `kode-leptos`.

### 5. Remove the bootstrap token

Once all four crates have Trusted Publishing configured, delete the
`CARGO_REGISTRY_TOKEN` repository secret. The workflow prefers OIDC when it's
available and only falls back to the secret if OIDC fails — removing the
secret makes OIDC the sole authentication path.

## Regular release (once bootstrapped)

### 1. Bump the version

All crates share a single version from `[workspace.package]` in the root
`Cargo.toml`. Edit it:

```toml
[workspace.package]
version = "0.2.0"  # was 0.1.0
```

Run `cargo check --workspace` so `Cargo.lock` updates. Update the
[CHANGELOG](CHANGELOG.md).

### 2. Commit, tag, push

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "Release v0.2.0"
git push

git tag v0.2.0
git push origin v0.2.0
```

The workflow runs on the tag push. `publish_if_new` skips crates whose new
version is already on crates.io, so partial re-runs are safe.

## Manual dry run

Before tagging, exercise the workflow end-to-end without actually publishing:

1. https://github.com/kyomi-ai/kode/actions/workflows/publish-crates.yml
2. **Run workflow** → set `dry_run` to `true` → **Run workflow**.
3. Each crate runs through `cargo publish --dry-run` and nothing lands on
   crates.io.

## Troubleshooting

**Workflow fails at "Get crates.io publish token via OIDC"**
: OIDC isn't configured for one or more crates yet. Add a `CARGO_REGISTRY_TOKEN`
  secret as described in bootstrap step 2 — the workflow falls back to it.

**`cargo publish` fails: "crate version is already uploaded"**
: You tried to publish a version that already exists. `publish_if_new` should
  catch this, but if it races, bump the version and re-tag.

**`cargo publish` fails: "no matching package named `kode-core` found"**
: A downstream crate is being published before its upstream dep finished
  propagating through the crates.io index. The workflow has a 60s sleep
  between crates — if it still races, bump the sleep in
  `.github/workflows/publish-crates.yml`.

**Yanking a release**
: `cargo yank --version 0.1.0 kode-core`. Yank every crate in the release,
  not just one. Yanking doesn't delete; it flags the version as unsafe for
  new `cargo` resolutions.
