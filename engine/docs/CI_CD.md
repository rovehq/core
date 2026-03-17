# CI/CD Documentation

This document describes the Continuous Integration and Continuous Deployment (CI/CD) setup for the Rove project.

## Overview

The Rove project uses GitHub Actions for CI/CD, with workflows that:

1. Test on all supported platforms (Linux, macOS, Windows)
2. Build release binaries for all platforms
3. Run security audits and code coverage
4. Track performance benchmarks
5. Automatically update dependencies

## Supported Platforms

### Testing Platforms

All code is tested on:

- **Linux**: Ubuntu latest (x86_64)
- **macOS**: Latest (x86_64 and ARM64)
- **Windows**: Latest (x86_64)

### Release Platforms

Release binaries are built for:

- **Linux x86_64**: `x86_64-unknown-linux-gnu`
- **macOS x86_64**: `x86_64-apple-darwin`
- **macOS ARM64**: `aarch64-apple-darwin` (Apple Silicon)
- **Windows x86_64**: `x86_64-pc-windows-msvc`

## Workflows

### 1. CI Workflow (`ci.yml`)

**Purpose**: Validate code quality and functionality on every push and pull request.

**Triggers**:
- Push to `main` or `develop` branches
- Pull requests to `main` or `develop` branches

**Jobs**:

#### Test Job
Runs on all three platforms (Linux, macOS, Windows) and performs:

1. **Code Formatting Check**
   ```bash
   cargo fmt --all -- --check
   ```
   Ensures all code follows Rust formatting standards.

2. **Clippy Lints**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```
   Runs Rust linter with zero warnings allowed.

3. **Build All Components**
   - Engine: `cargo build --package Rove-engine`
   - SDK: `cargo build --package sdk`
   - Core Tools: telegram, ui-server, api-server
   - Plugins (WASM): fs-editor, terminal, git, screenshot

4. **Run Tests**
   ```bash
   cargo test --all --verbose
   ```
   Runs all unit and integration tests.

#### Security Audit Job
Runs on Ubuntu and performs:

1. **Dependency Audit**
   ```bash
   cargo audit
   ```
   Checks for known security vulnerabilities in dependencies.

#### Coverage Job
Runs on Ubuntu and performs:

1. **Code Coverage**
   ```bash
   cargo tarpaulin --out Xml --verbose
   ```
   Generates code coverage report.

2. **Upload to Codecov**
   Uploads coverage data for tracking over time.

**Caching Strategy**:
- Cargo registry: `~/.cargo/registry`
- Cargo index: `~/.cargo/git`
- Build artifacts: `target/`

Cache keys include OS and `Cargo.lock` hash for optimal cache hits.

### 2. Release Workflow (`release.yml`)

**Purpose**: Build and publish release binaries for all platforms.

**Triggers**:
- Push of version tags (e.g., `v0.1.0`, `v1.2.3`)
- Manual workflow dispatch

**Jobs**:

#### Build Job
Runs on all platforms with a matrix strategy:

1. **Build Engine (Release)**
   ```bash
   cargo build --package engine--release --target $TARGET
   ```

2. **Build Core Tools (Release)**
   ```bash
   cargo build --package telegram --release --target $TARGET
   cargo build --package ui-server --release --target $TARGET
   cargo build --package api-server --release --target $TARGET
   ```

3. **Build Plugins (WASM)**
   ```bash
   cargo build --package fs-editor --release --target wasm32-wasip1
   cargo build --package terminal --release --target wasm32-wasip1
   cargo build --package git --release --target wasm32-wasip1
   cargo build --package screenshot --release --target wasm32-wasip1
   ```

4. **Package Artifacts**
   - Unix: Creates `.tar.gz` archives
   - Windows: Creates `.zip` archives
   
   Each archive contains:
   - `bin/Rove` (or `Rove.exe`)
   - `core-tools/*.so|.dylib|.dll`
   - `plugins/*.wasm`

5. **Upload Artifacts**
   Uploads platform-specific archives as workflow artifacts.

#### Create Release Job
Runs after all builds complete:

1. **Download Artifacts**
   Downloads all platform binaries.

2. **Generate Release Notes**
   Creates markdown release notes with:
   - Version information
   - Download links for each platform
   - Installation instructions
   - What's included

3. **Create GitHub Release**
   - Attaches all platform binaries
   - Publishes release notes
   - Tags the release

**Release Artifacts**:
- `Rove-linux-x86_64.tar.gz`
- `Rove-macos-x86_64.tar.gz`
- `Rove-macos-aarch64.tar.gz`
- `Rove-windows-x86_64.zip`

### 3. Dependencies Workflow (`dependencies.yml`)

**Purpose**: Keep dependencies up to date automatically.

**Triggers**:
- Weekly schedule (Monday at 9:00 AM UTC)
- Manual workflow dispatch

**Process**:

1. **Update Dependencies**
   ```bash
   cargo update
   ```

2. **Run Tests**
   ```bash
   cargo test --all
   ```

3. **Create Pull Request**
   If tests pass, creates a PR with:
   - Updated `Cargo.lock`
   - Descriptive commit message
   - Test results

### 4. Benchmark Workflow (`benchmark.yml`)

**Purpose**: Track performance over time and catch regressions.

**Triggers**:
- Push to `main` branch
- Pull requests to `main` branch
- Manual workflow dispatch

**Process**:

1. **Run Benchmarks**
   ```bash
   cargo criterion --message-format=json
   ```

2. **Store Results**
   Stores benchmark results for historical comparison.

3. **Alert on Regressions**
   - Threshold: 150% slower than baseline
   - Comments on PRs if regression detected
   - Does not fail the build (informational)

## Dependabot Configuration

Dependabot is configured to automatically create PRs for:

1. **Cargo Dependencies**
   - Weekly updates on Monday at 9:00 AM
   - Up to 10 open PRs
   - Labeled with `dependencies` and `rust`

2. **GitHub Actions**
   - Weekly updates on Monday at 9:00 AM
   - Up to 5 open PRs
   - Labeled with `dependencies` and `github-actions`

3. **npm Dependencies** (for lab)
   - Weekly updates on Monday at 9:00 AM
   - Up to 10 open PRs
   - Labeled with `dependencies` and `npm`

## Performance Requirements

The CI/CD system validates these performance requirements:

| Metric | Requirement | Validation Method |
|--------|-------------|-------------------|
| Binary Size | < 10MB | Release build check |
| Startup Time | < 2 seconds | Benchmark tests |
| Plugin Load Time | < 100ms | Benchmark tests |
| Core Tool Load Time | < 50ms | Benchmark tests |
| Idle Memory | < 100MB | Integration tests |
| Loaded Memory | < 500MB | Integration tests |

## Creating a Release

To create a new release:

1. **Update Version Numbers**
   ```bash
   # Update version in Cargo.toml files
   vim Rove-engine/Cargo.toml
   vim sdk/Cargo.toml
   # ... update other packages
   ```

2. **Commit Changes**
   ```bash
   git add .
   git commit -m "chore: bump version to v0.2.0"
   git push origin main
   ```

3. **Create and Push Tag**
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. **Wait for CI/CD**
   - GitHub Actions will automatically:
     - Build binaries for all platforms
     - Run all tests
     - Create a GitHub release
     - Upload all artifacts

5. **Verify Release**
   - Check GitHub Releases page
   - Download and test binaries
   - Update release notes if needed

## Local Testing

### Testing Workflows Locally

Use [act](https://github.com/nektos/act) to test workflows locally:

```bash
# Install act
brew install act  # macOS
# or
curl https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash  # Linux

# Run CI workflow
act push

# Run specific job
act -j test

# Run with specific platform
act -P ubuntu-latest=ghcr.io/catthehacker/ubuntu:act-latest
```

### Testing Builds Locally

```bash
# Test release build
cargo build --release --package Rove-engine

# Test cross-compilation (requires cross)
cargo install cross
cross build --target x86_64-unknown-linux-gnu --release
cross build --target x86_64-pc-windows-gnu --release

# Test WASM builds
rustup target add wasm32-wasip1
cargo build --package fs-editor --target wasm32-wasip1 --release
```

## Troubleshooting

### Build Failures

**Symptom**: Build fails on CI but works locally.

**Solutions**:
1. Check that `Cargo.lock` is committed
2. Verify all dependencies are specified correctly
3. Test with `cargo clean && cargo build`
4. Check platform-specific code with `#[cfg(target_os)]`

### Test Failures

**Symptom**: Tests fail on specific platforms.

**Solutions**:
1. Check for platform-specific assumptions (paths, line endings)
2. Use `#[cfg(test)]` for test-only code
3. Mock platform-specific functionality
4. Test locally on the failing platform

### Cache Issues

**Symptom**: Builds are slow or fail due to cache corruption.

**Solutions**:
1. Clear cache from GitHub Actions UI
2. Update cache key in workflow
3. Verify `Cargo.lock` is up to date

### Release Failures

**Symptom**: Release workflow fails to create release.

**Solutions**:
1. Ensure tag follows `v*.*.*` format
2. Check that all builds succeeded
3. Verify `GITHUB_TOKEN` permissions
4. Check for existing release with same tag

### WASM Build Failures

**Symptom**: Plugin builds fail on CI.

**Solutions**:
1. Ensure `wasm32-wasip1` target is installed
2. Check for platform-specific dependencies
3. Verify Extism PDK version compatibility
4. Test locally with `cargo build --target wasm32-wasip1`

## Security Considerations

### Secrets Management

- Never commit secrets to the repository
- Use GitHub Secrets for sensitive data
- Rotate secrets regularly
- Audit secret access logs

### Dependency Security

- Dependabot automatically checks for vulnerabilities
- `cargo audit` runs on every CI build
- Review dependency updates before merging
- Pin critical dependencies to specific versions

### Build Security

- All builds run in isolated environments
- No network access during builds (except dependency downloads)
- Artifacts are signed and verified
- Release binaries are built from tagged commits only

## Monitoring and Alerts

### Build Status

Monitor build status at:
- GitHub Actions tab
- Status badges in README
- Email notifications (configure in GitHub settings)

### Performance Tracking

Track performance at:
- Benchmark workflow results
- Historical benchmark data
- Performance regression alerts in PRs

### Dependency Updates

Monitor dependency updates:
- Dependabot PRs
- Security advisories
- Changelog reviews

## Best Practices

1. **Always run tests locally before pushing**
   ```bash
   cargo test --all
   cargo clippy --all-targets -- -D warnings
   cargo fmt --all -- --check
   ```

2. **Keep CI fast**
   - Use caching effectively
   - Run expensive tests only on main branch
   - Parallelize independent jobs

3. **Document breaking changes**
   - Update CHANGELOG.md
   - Add migration guides
   - Bump version appropriately

4. **Review CI failures promptly**
   - Fix failing tests immediately
   - Don't merge PRs with failing CI
   - Investigate flaky tests

5. **Keep workflows maintainable**
   - Use reusable workflows
   - Document complex steps
   - Keep workflow files DRY

## References

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Rust CI/CD Best Practices](https://doc.rust-lang.org/cargo/guide/continuous-integration.html)
- [Cross-Platform Rust Builds](https://rust-lang.github.io/rustup/cross-compilation.html)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
- [Extism Documentation](https://extism.org/docs/)
