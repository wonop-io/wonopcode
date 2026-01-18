# Security Model

The security model is designed to ensure that code execution is sandboxed, controlling access to files, network resources, and other sensitive operations.

## Overview
The system uses a permission-based approach, combined with a sandbox mechanism. For instance, if `permissions.read` is `true`, the environment can read files (within allowed sandbox paths). If `permissions.write` is `false`, writes are prohibited.

## How it Works
1. **Configuration** – The user supplies a config (see [Configuration Schema](../reference/config-schema.md)) specifying sandbox paths and permissions.
2. **Policy Enforcement** – At runtime, the environment checks every tool invocation (read, write, network) against the declared permissions.
3. **Sandbox Constraints** – Even if reading/writing is permitted, must ensure the file path is within an allowed directory. Access outside these paths is denied.

## Real-World Examples
### Example 1: Minimal Permissions
Your config might look like:
```json
{
  "permissions": {
    "read": true,
    "write": false,
    "network": false
  },
  "sandbox": {
    "enabled": true,
    "allowedPaths": ["/docs"],
    "deniedPaths": ["/secret"]
  }
}
```
• In this setup, the environment can **only** read within `/docs`.
• Any attempt to write or send network requests fails.

### Example 2: Partial Write Access
```json
{
  "permissions": {
    "read": true,
    "write": true,
    "network": false
  },
  "sandbox": {
    "enabled": true,
    "allowedPaths": ["/src", "/include"],
    "deniedPaths": ["/private", "/etc"]
  }
}
```
• The environment can read/write inside `/src` or `/include`, but cannot even read `/private` or `/etc`.
• No network allowed.

### Example 3: Network-Enabled Testing
```json
{
  "permissions": {
    "read": true,
    "write": true,
    "network": true
  },
  "sandbox": {
    "enabled": false
  }
}
```
• This example disables the sandbox, allowing access to any path.
• It also enables network requests (e.g., to call external APIs). Use with caution!

## Custom Permission Rules
One might create a "lowest-privilege" approach by default, then instruct developers to override only as necessary. For example, local configuration might set `network=true` for integration tests but remain off in production.

## Additional Considerations
• **User Discretion** – Even with tight config, if you run code that discards these checks, it could be unsafe.
• **Shared Machines** – For multi-user systems, consider separate containers or VMs for ultimate isolation.
• **Development vs Production** – In dev mode, you might enable more permissions to expedite debugging and iteration. In production, narrower permissions are recommended.

## Future Plans
• More granular file-level rules (e.g., `/src/*.rs` writeable, but `/src/lib.rs` read-only).
• Possibly integrate with container-based isolation.
• Extended network permission policies (e.g., restricting certain domains).

For more details on how config merges with environment variables, see [Configuration Schema](../reference/config-schema.md).

_End of Security Model_
