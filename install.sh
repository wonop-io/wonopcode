#!/bin/bash
# Wonopcode installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/wonop-io/wonopcode/main/install.sh | bash
#    or: curl -fsSL https://raw.githubusercontent.com/wonop-io/wonopcode/main/install.sh | bash -s -- --version v0.1.0
#
# Environment variables:
#   WONOPCODE_INSTALL_DIR - Installation directory (default: ~/.local/bin or /usr/local/bin)
#   WONOPCODE_VERSION     - Specific version to install (default: latest)

set -euo pipefail

# Configuration
REPO="wonop-io/wonopcode"
BINARY_NAME="wonopcode"
GITHUB_API="https://api.github.com"
GITHUB_RELEASES="https://github.com/${REPO}/releases"

# Colors (disabled if not a terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    BOLD='\033[1m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    BOLD=''
    NC=''
fi

# Logging functions
info() {
    echo -e "${BLUE}==>${NC} ${BOLD}$1${NC}"
}

success() {
    echo -e "${GREEN}==>${NC} ${BOLD}$1${NC}"
}

warn() {
    echo -e "${YELLOW}warning:${NC} $1"
}

error() {
    echo -e "${RED}error:${NC} $1" >&2
}

die() {
    error "$1"
    exit 1
}

# Parse arguments
VERSION="${WONOPCODE_VERSION:-}"
INSTALL_DIR="${WONOPCODE_INSTALL_DIR:-}"

while [[ $# -gt 0 ]]; do
    case $1 in
        --version|-v)
            VERSION="$2"
            shift 2
            ;;
        --dir|-d)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --help|-h)
            echo "Wonopcode Installer"
            echo ""
            echo "Usage: curl -fsSL https://raw.githubusercontent.com/wonop-io/wonopcode/main/install.sh | bash"
            echo "   or: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -v, --version VERSION  Install a specific version (default: latest)"
            echo "  -d, --dir DIR          Installation directory"
            echo "  -h, --help             Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  WONOPCODE_VERSION      Specific version to install"
            echo "  WONOPCODE_INSTALL_DIR  Installation directory"
            exit 0
            ;;
        *)
            warn "Unknown option: $1"
            shift
            ;;
    esac
done

# Detect OS and architecture
detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux*)
            os="linux"
            ;;
        Darwin*)
            os="darwin"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            os="windows"
            ;;
        *)
            die "Unsupported operating system: $os"
            ;;
    esac

    case "$arch" in
        x86_64|amd64)
            arch="x86_64"
            ;;
        arm64|aarch64)
            arch="aarch64"
            ;;
        *)
            die "Unsupported architecture: $arch"
            ;;
    esac

    echo "${os}-${arch}"
}

# Get the latest release version from GitHub
get_latest_version() {
    local latest_url="${GITHUB_API}/repos/${REPO}/releases/latest"
    
    if command -v curl &> /dev/null; then
        curl -fsSL "$latest_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    elif command -v wget &> /dev/null; then
        wget -qO- "$latest_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    else
        die "Neither curl nor wget found. Please install one of them."
    fi
}

# Download a file
download() {
    local url="$1"
    local output="$2"

    info "Downloading from $url"

    if command -v curl &> /dev/null; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget &> /dev/null; then
        wget -q "$url" -O "$output"
    else
        die "Neither curl nor wget found. Please install one of them."
    fi
}

# Verify checksum
verify_checksum() {
    local file="$1"
    local checksums_file="$2"
    local expected_name="$3"

    if [ ! -f "$checksums_file" ]; then
        warn "Checksums file not found, skipping verification"
        return 0
    fi

    local expected_sum
    expected_sum=$(grep "$expected_name" "$checksums_file" | awk '{print $1}')
    
    if [ -z "$expected_sum" ]; then
        warn "Checksum not found for $expected_name, skipping verification"
        return 0
    fi

    local actual_sum
    if command -v sha256sum &> /dev/null; then
        actual_sum=$(sha256sum "$file" | awk '{print $1}')
    elif command -v shasum &> /dev/null; then
        actual_sum=$(shasum -a 256 "$file" | awk '{print $1}')
    else
        warn "sha256sum/shasum not found, skipping checksum verification"
        return 0
    fi

    if [ "$expected_sum" != "$actual_sum" ]; then
        die "Checksum verification failed!
Expected: $expected_sum
Actual:   $actual_sum"
    fi

    info "Checksum verified"
}

# Extract the archive
extract() {
    local archive="$1"
    local dest="$2"

    case "$archive" in
        *.tar.gz|*.tgz)
            tar -xzf "$archive" -C "$dest"
            ;;
        *.zip)
            unzip -q "$archive" -d "$dest"
            ;;
        *)
            die "Unknown archive format: $archive"
            ;;
    esac
}

# Determine installation directory
get_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        echo "$INSTALL_DIR"
        return
    fi

    # Prefer ~/.local/bin if it exists or can be created
    local local_bin="$HOME/.local/bin"
    if [ -d "$local_bin" ] || mkdir -p "$local_bin" 2>/dev/null; then
        echo "$local_bin"
        return
    fi

    # Fall back to /usr/local/bin (requires sudo)
    echo "/usr/local/bin"
}

# Check if directory is in PATH
check_path() {
    local dir="$1"
    
    if [[ ":$PATH:" != *":$dir:"* ]]; then
        warn "$dir is not in your PATH"
        echo ""
        echo "Add it to your shell configuration:"
        echo ""
        
        local shell_name
        shell_name=$(basename "$SHELL")
        
        case "$shell_name" in
            bash)
                echo "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.bashrc"
                echo "  source ~/.bashrc"
                ;;
            zsh)
                echo "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc"
                echo "  source ~/.zshrc"
                ;;
            fish)
                echo "  fish_add_path $dir"
                ;;
            *)
                echo "  export PATH=\"$dir:\$PATH\""
                ;;
        esac
        echo ""
    fi
}

# Main installation function
main() {
    echo ""
    echo -e "${BOLD}Wonopcode Installer${NC}"
    echo ""

    # Detect platform
    local platform
    platform=$(detect_platform)
    info "Detected platform: $platform"

    # Get version
    if [ -z "$VERSION" ]; then
        info "Fetching latest version..."
        VERSION=$(get_latest_version)
        if [ -z "$VERSION" ]; then
            die "Failed to determine latest version"
        fi
    fi
    info "Version: $VERSION"

    # Determine artifact name and extension
    local artifact_name extension
    case "$platform" in
        linux-x86_64)
            artifact_name="wonopcode-linux-x86_64"
            extension="tar.gz"
            ;;
        linux-aarch64)
            artifact_name="wonopcode-linux-aarch64"
            extension="tar.gz"
            ;;
        darwin-x86_64)
            artifact_name="wonopcode-darwin-x86_64"
            extension="tar.gz"
            ;;
        darwin-aarch64)
            artifact_name="wonopcode-darwin-aarch64"
            extension="tar.gz"
            ;;
        windows-x86_64)
            artifact_name="wonopcode-windows-x86_64"
            extension="zip"
            ;;
        *)
            die "No pre-built binary available for $platform"
            ;;
    esac

    # Create temporary directory
    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    # Download archive and checksums
    local archive_name="${artifact_name}.${extension}"
    local archive_path="${tmp_dir}/${archive_name}"
    local checksums_path="${tmp_dir}/checksums.txt"

    local download_url="${GITHUB_RELEASES}/download/${VERSION}/${archive_name}"
    local checksums_url="${GITHUB_RELEASES}/download/${VERSION}/checksums.txt"

    download "$download_url" "$archive_path"
    download "$checksums_url" "$checksums_path" || warn "Failed to download checksums file"

    # Verify checksum
    verify_checksum "$archive_path" "$checksums_path" "$archive_name"

    # Extract archive
    info "Extracting archive..."
    local extract_dir="${tmp_dir}/extract"
    mkdir -p "$extract_dir"
    extract "$archive_path" "$extract_dir"

    # Find the binary
    local binary_path
    if [ "$platform" = "windows-x86_64" ]; then
        binary_path=$(find "$extract_dir" -name "${BINARY_NAME}.exe" -type f | head -1)
    else
        binary_path=$(find "$extract_dir" -name "$BINARY_NAME" -type f | head -1)
    fi

    if [ -z "$binary_path" ] || [ ! -f "$binary_path" ]; then
        die "Binary not found in archive"
    fi

    # Determine install directory
    local install_dir
    install_dir=$(get_install_dir)
    info "Installing to: $install_dir"

    # Create install directory if needed
    if [ ! -d "$install_dir" ]; then
        mkdir -p "$install_dir" 2>/dev/null || sudo mkdir -p "$install_dir"
    fi

    # Install binary
    local dest_path="${install_dir}/${BINARY_NAME}"
    if [ "$platform" = "windows-x86_64" ]; then
        dest_path="${install_dir}/${BINARY_NAME}.exe"
    fi

    if [ -w "$install_dir" ]; then
        cp "$binary_path" "$dest_path"
        chmod +x "$dest_path"
    else
        info "Requesting sudo access to install to $install_dir"
        sudo cp "$binary_path" "$dest_path"
        sudo chmod +x "$dest_path"
    fi

    echo ""
    success "Wonopcode $VERSION installed successfully!"
    echo ""

    # Check PATH
    check_path "$install_dir"

    # Verify installation
    if command -v "$BINARY_NAME" &> /dev/null; then
        echo "Run 'wonopcode --help' to get started"
    else
        echo "Run '$dest_path --help' to get started"
    fi
    echo ""
}

main "$@"
