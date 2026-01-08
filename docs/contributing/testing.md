# Testing Guide

This guide explains how to run and write tests for the project.

## Overview
To ensure that the code remains stable and correct, we maintain a suite of unit tests, integration tests, and end-to-end tests (where applicable). Contributors should run the test suite before submitting changes.

### Command-line Testing
Most tests can be run with:
```
cargo test
```
Add additional flags if needed:
```
cargo test --features integration
```

Depending on the crate, you can also specify which package to test:
```
cargo test -p wonopcode-core
```

## Configuration References
Some tests rely on environment variables or the config schema. Please see [Configuration Schema](../reference/config-schema.md) for details.

• If you have certain environment variables defined (e.g., `LOG_LEVEL="debug"`), it may affect test verbosity.
• Permissions can also be toggled via config in local project settings, which can influence tests.

## Troubleshooting
Here are a few common issues you might encounter:

1. **Tests failing in CI but passing locally**
   - Check for environment differences (e.g. missing environment variables, different cargo or rustc versions, or OS-level dependencies).
   - Some tests might rely on external services or tools that are not available in CI.

2. **Networking tests timing out**
   - Ensure that `permissions.network` is enabled in either your config or environment when running.
   - Verify your internet connection or local DNS settings.

3. **Permission errors**
   - If sandbox is incorrectly configured, file read/write tests may fail.
   - See the "permissions" setting in [Configuration Schema](../reference/config-schema.md) and confirm the relevant flags.

4. **Docker-based testing issues**
   - Check Docker version and resources if the tests are containerized.
   - Some directories may be mapped incorrectly if sandbox paths or volumes mismatch.

5. **Feature flag confusion**
   - For crates that have multiple optional features, ensure you enable them when required or see if any feature is conflicting with test assumptions.

## Further Resources
• [Development Setup](development-setup.md) for environment configuration.
• [Contributing Guide](CONTRIBUTING.md) for general guidelines on pull requests and coding standards.

_End of Testing Guide_
