# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in Quicksilver, please report it responsibly.

### How to Report

1. **Do NOT** open a public GitHub issue for security vulnerabilities
2. Email the maintainers directly (see repository for contact info)
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Any suggested fixes

### What to Expect

- Acknowledgment within 48 hours
- Regular updates on progress
- Credit in the security advisory (unless you prefer anonymity)

### Scope

Security issues we're interested in:
- Memory safety violations
- Sandbox escapes
- Capability bypass
- Code execution vulnerabilities
- Denial of service (resource exhaustion)

### Security Design

Quicksilver is designed with security in mind:

1. **Memory Safety**: Pure Rust implementation eliminates buffer overflows, use-after-free, and other memory corruption vulnerabilities common in C/C++ runtimes.

2. **Capability-Based Security**: The sandbox system (`src/security/mod.rs`) provides fine-grained control over:
   - File system access (read/write with path patterns)
   - Network access (host patterns, IP ranges)
   - Environment variables
   - Subprocess execution
   - Dynamic code evaluation

3. **Resource Limits**: The VM enforces:
   - Maximum stack size (10,000 entries)
   - Maximum call depth (1,000 frames)
   - Configurable execution timeouts

### Known Limitations

The following are known and documented, not vulnerabilities:
- `WeakMap`/`WeakSet` use strong references (memory, not security)
- Some JavaScript features are stubs (clearly documented)
- Network fetch is not yet implemented

## Security Updates

Security fixes will be released as patch versions (e.g., 0.1.1) and announced via:
- GitHub Releases
- CHANGELOG.md

## Acknowledgments

We appreciate responsible security researchers who help keep Quicksilver safe.
