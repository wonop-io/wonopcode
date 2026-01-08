# Wonopcode Sandbox Container Image
#
# A lightweight development environment for sandboxed code execution.
# Based on Alpine Linux for minimal footprint.
#
# Build:
#   docker build -t wonopcode/sandbox:latest .
#
# Run:
#   docker run -it --rm -v $(pwd):/workspace wonopcode/sandbox:latest

FROM alpine:3.19

LABEL org.opencontainers.image.title="Wonopcode Sandbox"
LABEL org.opencontainers.image.description="Sandboxed execution environment for AI code generation"
LABEL org.opencontainers.image.source="https://github.com/wonop/wonopcode"
LABEL wonopcode.sandbox="true"

# Install base packages
RUN apk add --no-cache \
    # Shell
    bash \
    zsh \
    # Core utilities
    coreutils \
    findutils \
    grep \
    sed \
    gawk \
    # Development tools
    git \
    curl \
    wget \
    jq \
    yq \
    # Build essentials
    build-base \
    linux-headers \
    # Search tools (ripgrep, fd)
    ripgrep \
    fd \
    # File utilities
    tree \
    file \
    less \
    # Process utilities
    procps \
    # Network utilities
    openssh-client \
    ca-certificates

# Install Python
RUN apk add --no-cache \
    python3 \
    py3-pip \
    py3-setuptools \
    py3-wheel

# Install Node.js (LTS)
RUN apk add --no-cache \
    nodejs \
    npm

# Install common Python packages
RUN pip3 install --no-cache-dir --break-system-packages \
    # HTTP clients
    requests \
    httpx \
    # Data formats
    pyyaml \
    toml \
    # Development tools
    black \
    ruff \
    mypy \
    # Testing
    pytest \
    # Utilities
    python-dotenv \
    click

# Install common Node packages globally
RUN npm install -g \
    typescript \
    ts-node \
    eslint \
    prettier \
    @types/node

# Create workspace directory
RUN mkdir -p /workspace
WORKDIR /workspace

# Create cache directories
RUN mkdir -p \
    /root/.cache/pip \
    /root/.npm \
    /root/.cargo/registry

# Environment setup
ENV TERM=dumb
ENV NO_COLOR=1
ENV GIT_TERMINAL_PROMPT=0
ENV DEBIAN_FRONTEND=noninteractive
ENV PATH="/root/.local/bin:$PATH"
ENV PYTHONUNBUFFERED=1
ENV NODE_ENV=development

# Disable git safe directory warnings
RUN git config --global --add safe.directory '*'

# Set shell
SHELL ["/bin/bash", "-c"]

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD echo "healthy" || exit 1

# Default command - keep container running
CMD ["sleep", "infinity"]
