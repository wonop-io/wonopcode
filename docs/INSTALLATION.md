# Installation

Complete guide to installing wonopcode on your system.

---

## Requirements

### Minimum Requirements

- **OS**: macOS 12+, Linux (glibc 2.31+)
- **Architecture**: x86_64 or ARM64
- **Rust**: 1.75+ (for building from source)

### Optional Requirements

- **Docker/Podman**: Required for sandboxed execution
- **Lima**: Alternative sandbox runtime on macOS
- **ripgrep**: Enhanced search (bundled, but can use system version)

---

## Installation Methods

### From Source (Recommended)

Building from source ensures you get the latest version and optimal performance for your system.

```bash
# Clone the repository
git clone https://github.com/wonop-io/wonopcode
cd wonopcode

# Build in release mode (optimized)
cargo build --release

# Binary is at ./target/release/wonopcode
```

#### Install to PATH

**Option 1**: Copy to a directory in your PATH

```bash
sudo cp target/release/wonopcode /usr/local/bin/
```

**Option 2**: Add the build directory to PATH

```bash
# Add to ~/.bashrc or ~/.zshrc
export PATH="$PATH:$HOME/path/to/wonopcode/target/release"
```

**Option 3**: Create a symlink

```bash
ln -s $(pwd)/target/release/wonopcode ~/.local/bin/wonopcode
```

### Using Cargo Install

```bash
cargo install --path crates/wonopcode
```

Or from the workspace root:

```bash
cargo install --path .
```

---

## Platform-Specific Notes

### macOS

#### Apple Silicon (M1/M2/M3)

Native ARM64 build for best performance:

```bash
cargo build --release
```

#### Intel Macs

Standard x86_64 build:

```bash
cargo build --release
```

#### Xcode Command Line Tools

If you see build errors, ensure Xcode tools are installed:

```bash
xcode-select --install
```

#### Sandbox Runtime (Lima)

For macOS sandboxing without Docker Desktop:

```bash
brew install lima
limactl start
```

### Linux

#### Ubuntu/Debian

Install build dependencies:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev
```

Then build:

```bash
cargo build --release
```

#### Fedora/RHEL

```bash
sudo dnf install -y gcc pkg-config openssl-devel
cargo build --release
```

#### Arch Linux

```bash
sudo pacman -S base-devel openssl
cargo build --release
```

#### Sandbox Runtime (Docker)

```bash
# Install Docker
curl -fsSL https://get.docker.com | sh

# Add your user to the docker group
sudo usermod -aG docker $USER

# Log out and back in, then verify
docker run hello-world
```

#### Sandbox Runtime (Podman)

Rootless alternative to Docker:

```bash
# Ubuntu/Debian
sudo apt install podman

# Fedora
sudo dnf install podman
```

### Windows (Coming Soon)

Windows support is in development. Current options:

1. **WSL2**: Run wonopcode in Windows Subsystem for Linux
2. **Native**: Coming in a future release

#### WSL2 Installation

```bash
# In WSL2 (Ubuntu)
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/wonop-io/wonopcode
cd wonopcode
cargo build --release
```

---

## Verifying Installation

After installation, verify wonopcode works:

```bash
# Check version
wonopcode --version

# Check help
wonopcode --help

# Start (requires API key)
export ANTHROPIC_API_KEY="your-key"
wonopcode
```

Expected output for `--version`:

```
wonopcode 0.1.0
```

---

## Setting Up API Keys

Wonopcode requires an API key from at least one provider.

### Environment Variables

| Provider | Environment Variable |
|----------|---------------------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| Google | `GOOGLE_API_KEY` |
| OpenRouter | `OPENROUTER_API_KEY` |
| Azure OpenAI | `AZURE_OPENAI_API_KEY` |
| AWS Bedrock | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` |
| xAI | `XAI_API_KEY` |
| Mistral | `MISTRAL_API_KEY` |
| Groq | `GROQ_API_KEY` |

### Persisting API Keys

Add to your shell configuration:

**Bash** (`~/.bashrc`):
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Zsh** (`~/.zshrc`):
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Fish** (`~/.config/fish/config.fish`):
```fish
set -gx ANTHROPIC_API_KEY "sk-ant-..."
```

---

## Configuration Directory

Wonopcode stores configuration and data in standard locations:

| Platform | Config Directory |
|----------|-----------------|
| macOS | `~/.config/wonopcode/` |
| Linux | `~/.config/wonopcode/` |
| Windows | `%APPDATA%\wonopcode\` |

Create the directory:

```bash
mkdir -p ~/.config/wonopcode
```

Minimal configuration file (`~/.config/wonopcode/config.json`):

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929"
}
```

→ See [Configuration](./CONFIGURATION.md) for all options.

---

## Installing Sandbox Runtime

Sandboxing is optional but highly recommended for safe AI code execution.

### Docker (Recommended for Linux)

```bash
# Install Docker
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER

# Verify (after logging out/in)
docker run hello-world
```

### Docker Desktop (macOS)

Download from [docker.com](https://www.docker.com/products/docker-desktop/) and install.

### Podman (Rootless Alternative)

```bash
# Ubuntu/Debian
sudo apt install podman

# Fedora
sudo dnf install podman

# Verify
podman run hello-world
```

### Lima (macOS Native)

```bash
brew install lima
limactl start

# Verify
limactl shell default uname -a
```

---

## Updating

### From Source

```bash
cd wonopcode
git pull
cargo build --release
```

### Cargo Install

```bash
cargo install --path crates/wonopcode --force
```

---

## Uninstalling

### Remove Binary

```bash
# If installed to /usr/local/bin
sudo rm /usr/local/bin/wonopcode

# If installed via cargo
cargo uninstall wonopcode
```

### Remove Configuration

```bash
rm -rf ~/.config/wonopcode
```

### Remove Data

```bash
rm -rf ~/.local/share/wonopcode
```

---

## Troubleshooting

### Build Errors

#### "linker not found"

Install build tools:

```bash
# macOS
xcode-select --install

# Ubuntu/Debian
sudo apt install build-essential

# Fedora
sudo dnf install gcc
```

#### "openssl not found"

Install OpenSSL development files:

```bash
# Ubuntu/Debian
sudo apt install libssl-dev

# Fedora
sudo dnf install openssl-devel

# macOS (usually not needed)
brew install openssl
```

#### "pkg-config not found"

```bash
# Ubuntu/Debian
sudo apt install pkg-config

# Fedora
sudo dnf install pkgconf

# macOS
brew install pkg-config
```

### Runtime Errors

#### "API key not found"

Ensure environment variable is set:

```bash
echo $ANTHROPIC_API_KEY
```

#### "Docker socket permission denied"

Add your user to the docker group:

```bash
sudo usermod -aG docker $USER
# Log out and back in
```

#### "libc version too old"

Your system's glibc is too old. Options:
1. Update your Linux distribution
2. Build with musl: `cargo build --release --target x86_64-unknown-linux-musl`

---

## Next Steps

- [Getting Started](./GETTING_STARTED.md) - Your first session
- [Configuration](./CONFIGURATION.md) - Customize your setup
- [Sandboxing](./SANDBOXING.md) - Enable secure execution
