# Contributing Guide

Welcome! We appreciate your interest in contributing to the project. This document outlines our contribution guidelines.

## Getting Started
• First, read [Development Setup](development-setup.md) for environment configuration and known issues.
• Check our [Testing Guide](testing.md) for how to run tests.
• See [Configuration Schema](../reference/config-schema.md) for environment variables and project settings.

## Pull Requests
1. **Fork** the repo
2. **Create a branch** for your changes
3. **Commit** with clear messages
4. Open a **Pull Request** against the main branch
5. Ensure your PR includes tests if relevant
6. Reference any open issues by ID in your PR description

## Coding Standards
• Rust code should follow rustfmt conventions and Clippy suggestions.
• Write descriptive commit messages.

## Documentation
• Please update or add documentation for features or changes.
• Some docs might contain placeholders (like [Tools API](../reference/tools-api.md) was incomplete), but we’re in the process of improving them. Feel free to contribute expansions.
• The same goes for the config schema. If you add new config keys or environment variables, update [config-schema.md](../reference/config-schema.md).

## Reporting Issues
• If you find a bug, please open a GitHub issue.
• Provide as much detail as possible—OS, Rust version, logs, steps to reproduce, etc.

## Additional Resources
• [Security Model](../architecture/security-model.md) – discusses how the project handles sandboxing and permissions.
• [Architecture Overview](../architecture/architecture-overview.md) – high-level design of crates.

## Thank You
We value every contribution, from filing issues to resolving them. Let’s build a robust, secure, and user-friendly system together!

_End of Contributing Guide_
