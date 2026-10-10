# Team Configuration

- **Team key:** KOD
- **Team name:** Kode
- **GitHub repo:** kyomi-ai/kode

## Versioning

- **Method:** semver
- **Tag format:** vMAJOR.MINOR.PATCH
- **Version source:** cargo-toml (workspace version in Cargo.toml is source of truth, must match tag)

## Release

- **Pipelines:** publish-crates.yml
- **Production URL:** (library — no deployment)
- **Post-release:** crates.io publish triggered by tag push (4 crates in dependency order, 60s between tiers)

## Review

- **Review mode:** pre-release (review before tagging, not after)
- **How to verify:** Build and serve the demo site from main (`trunk serve --address 0.0.0.0 --port 8090` in `demo/`), then test there
- **Skip release gate:** true (PRs on main are reviewable immediately — release happens after human review, not before)
