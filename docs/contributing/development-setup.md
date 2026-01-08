# Development Setup

This guide helps contributors set up their local environment to develop on the project.

## Prerequisites
• Install Rust (latest stable) and Cargo
• Ensure you have any external build dependencies installed (e.g., OpenSSL, libssl-dev)
• Optionally, Docker for container-based testing.

## Steps
1. Clone the repository:
```
git clone https://github.com/example/wonopcode.git
cd wonopcode
```
2. Build:
```
cargo build
```
3. Test:
```
cargo test
```

## Known Issues / FAQ
Below are some common hiccups encountered by contributors:

1. **SSL Link Errors on macOS**
   - Some macOS systems may require installing Command Line Tools for Xcode or additional homebrew libraries like `openssl@1.1`.
   - Try: `brew install openssl@1.1` and ensure `pkg-config` can find it.

2. **Docker is Not Installed**
   - Some advanced tests or local dev scripts rely on Docker. If Docker is unavailable, these tests will be skipped or fail.
   - If you cannot install Docker, check the CI logs or explore non-Docker alternatives.

3. **Filesystem Ownership Issues**
   - When using containers on macOS or Linux, sometimes file permissions can mismatch, leaving root ownership on certain volumes.
   - If this happens, run `sudo chown -R $(whoami) .` in the project directory to fix.

4. **Rust Toolchain Compatibility**
   - Check your default Rust version by running `rustc --version`. If you are on an older toolchain, you may have compilation issues.
   - The project typically tracks the latest stable Rust. Use `rustup update` to stay current.

5. **Networking or Proxy Problems**
   - Some corporate networks or proxies can block external requests needed for dependencies or submodules.
   - Check your proxy settings or consult your local IT.

6. **Large Logs**
   - If running with `LOG_LEVEL=debug` or higher, you may see verbose logs.
   - Use environment variables or config overrides to reduce log level.

## Additional Tips
• If you run across new issues not documented here, please open an issue or PR.
• For more detailed info on environment variables and permissions, see the [Configuration Schema](../reference/config-schema.md).

_End of Development Setup_
