# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`pgtls` is a protocol-aware TLS termination proxy for PostgreSQL written in Rust. It solves the problem that standard TLS proxies cannot handle PostgreSQL's `STARTTLS`-like TLS negotiation mechanism. The proxy understands the PostgreSQL wire protocol, handles `SSLRequest` messages correctly, and terminates TLS connections from clients before forwarding them as plaintext to backend servers.

## Key Architecture Concepts

### Protocol Handling
- **SSLRequest Detection**: The proxy identifies 8-byte `SSLRequest` messages (length=8, code=80877103) vs regular `StartupMessage` packets
- **State Machine**: Each connection follows: `AwaitingInitialBytes` → `SSLRequestDetected` → `RespondedToClient` → `ClientTlsHandshake` → `PlaintextForwarding`
- **TLS Termination**: Acts as TLS server to clients and forwards traffic as plaintext to backend servers

### Configuration Structure
- Multiple `[[proxy]]` routes in TOML configuration
- Each route maps one listener to one backend
- Supports TLS-to-plaintext mode only (simplified architecture)
- mTLS support for client connections

### Certificate Management
- Client-facing: Server certificate/key, optional client CA for mTLS verification
- Backend connections are plaintext (no certificates needed)
- Uses `rustls` with `tokio-rustls` integration

## Development Commands

### Building and Testing
```bash
# Build the project
make build
# or
cargo build

# Run tests (uses nextest)
make test
# or
cargo nextest run --all-features

# Format code
cargo fmt --all -- --check

# Lint code
cargo clippy --all-targets --all-features --tests --benches -- -D warnings

# Check code without building
cargo check --all
```

### Release Process
```bash
# Create release (updates changelog, tags, pushes)
make release
# This runs:
# - cargo release tag --execute
# - git cliff -o CHANGELOG.md (changelog generation)
# - git commit and push
# - cargo release push --execute
```

## Project Structure

### Core Directories
- `src/`: Main source code (currently minimal - project is in early development)
- `specs/`: Detailed architectural specifications (001-architectural-overview.md, 002-protocol-handling.md, etc.)
- `tasks/`: Implementation task breakdown following TDD approach
- `examples/`: Usage examples
- `fixtures/`: Test fixtures and data

### Specification Documents
The `specs/` directory contains the complete technical architecture:
- **001-architectural-overview.md**: High-level design and connection flow
- **002-protocol-handling.md**: PostgreSQL protocol parsing and state machine
- **003-tls-and-certificate-management.md**: TLS configuration with rustls
- **004-configuration.md**: TOML configuration schema and validation
- **005-logging-and-observability.md**: Logging strategy
- **006-cli.md**: Command-line interface

### Implementation Tasks
The `tasks/` directory outlines the TDD implementation plan:
1. Project setup and configuration parsing
2. Protocol parsing and SSLRequest detection
3. Connection handler (TLS-to-plaintext)
4. Connection handler (TLS-to-TLS)
5. Integration testing
6. Application assembly and logging

## Development Status

This is an early-stage project. The current `src/lib.rs` contains only a placeholder test. Implementation should follow the task sequence defined in `tasks/README.md` using a Test-Driven Development approach.

## Key Dependencies and Tools

- **Runtime**: `tokio` for async I/O
- **TLS**: `rustls` and `tokio-rustls` for TLS handling
- **Configuration**: `serde` and `toml` for TOML parsing
- **Testing**: `nextest` for test execution
- **Changelog**: `git-cliff` for automated changelog generation
- **CI/CD**: GitHub Actions with Rust toolchain, formatting, linting, and testing

## Configuration Example

```toml
log_level = "info"

[[proxy]]
  [proxy.listener]
  bind_address = "0.0.0.0:6432"
  server_cert = "/path/to/server.pem"
  server_key = "/path/to/server.key"
  mtls = true
  client_ca = "/path/to/client-ca.pem"

  [proxy.backend]
  address = "db.example.com:5432"
```

Usage:
```bash
pgtls -c config.toml
# or
pgtls --config config.toml
```

## Testing Strategy

Follow the TDD approach outlined in the tasks:
1. Write tests for configuration parsing and validation
2. Implement and test SSLRequest detection logic
3. Test TLS-to-plaintext connection handling
4. End-to-end integration tests with real PostgreSQL connections
5. Performance and security testing

When implementing, prioritize protocol correctness and security over performance optimizations in early stages.


# Rules

Build maintainable, scalable, and clean code by following KISS, YANGNI, SOLID principles

---

## KISS Principle
**Keep It Simple, Stupid**

### Core Rule
Always choose the simplest solution that effectively solves the problem.

### Guidelines
- **Favor clarity over cleverness**: Write code that others (including future you) can easily understand
- **Avoid premature optimization**: Don't optimize until you have a proven performance problem
- **Use straightforward algorithms**: Choose well-known, simple algorithms over complex ones unless complexity is justified
- **Minimize dependencies**: Only add external libraries when they provide significant value
- **Write self-documenting code**: Use clear variable names and function names that explain their purpose

---

## YAGNI Principle
**You Aren't Gonna Need It**

### Core Rule
Don't implement functionality until you actually need it.

### Guidelines
- **Build for current requirements**: Don't add features "just in case"
- **Avoid speculative generality**: Don't create abstract frameworks for hypothetical future needs
- **Delete unused code**: Remove dead code, unused methods, and unnecessary abstractions
- **Defer decisions**: Make design decisions as late as possible when you have more information
- **Focus on MVP**: Build the minimum viable product first, then iterate

### Red Flags (YAGNI Violations)
- Adding configuration options "for flexibility" that no one requested
- Creating abstract base classes with only one concrete implementation
- Building complex plugin systems before you have multiple plugins
- Adding database fields "we might need later"
- Creating utility functions before you have multiple use cases

---

## SOLID Principles

### S - Single Responsibility Principle (SRP)
Every class should have only one reason to change.

#### Rules:
- Each class should have one job
- If you can describe a class with "and" or "or", it probably violates SRP
- Changes to one aspect of functionality should only require changes to one class

### O - Open/Closed Principle (OCP)
Software entities should be open for extension but closed for modification.

#### Rules:
- Use inheritance, composition, or interfaces to extend behavior
- Don't modify existing code to add new features
- Design classes so new functionality can be added without changing existing code

### L - Liskov Substitution Principle (LSP)
Objects of a superclass should be replaceable with objects of its subclasses without breaking functionality.

#### Rules:
- Subclasses must be substitutable for their base classes
- Subclasses shouldn't strengthen preconditions or weaken postconditions
- Subclasses shouldn't throw exceptions that the base class doesn't throw

### I - Interface Segregation Principle (ISP)
No client should be forced to depend on methods it does not use.

#### Rules:
- Create small, focused interfaces
- Don't force classes to implement methods they don't need
- Split large interfaces into smaller, more specific ones

### D - Dependency Inversion Principle (DIP)
High-level modules should not depend on low-level modules. Both should depend on abstractions.

#### Rules:
- Depend on interfaces, not concrete implementations
- Inject dependencies rather than creating them internally
- High-level classes shouldn't know implementation details of low-level classes

---

## Implementation Guidelines

### Code Review Checklist

#### KISS Compliance
- [ ] Is the solution as simple as possible while still being correct?
- [ ] Can a new team member understand this code quickly?
- [ ] Are variable and function names self-explanatory?
- [ ] Is there unnecessary complexity that can be removed?

#### YAGNI Compliance
- [ ] Is every piece of code solving a current, real requirement?
- [ ] Are there any "just in case" features that can be removed?
- [ ] Is the abstraction level appropriate for current needs?
- [ ] Can any configuration options or parameters be removed?

#### SOLID Compliance
- [ ] Does each class have a single, clear responsibility? (SRP)
- [ ] Can new functionality be added without modifying existing code? (OCP)
- [ ] Are subclasses truly substitutable for their parents? (LSP)
- [ ] Are interfaces focused and cohesive? (ISP)
- [ ] Do high-level modules depend on abstractions, not concretions? (DIP)

### Best Practices Summary

1. **Start simple, evolve gradually**: Begin with the simplest solution and refactor as requirements become clearer
2. **Measure before optimizing**: Don't optimize for performance or flexibility until you have evidence it's needed
3. **Prefer composition over inheritance**: Use composition to achieve flexibility while maintaining simplicity
4. **Write tests first**: Tests help ensure you're building only what's needed and guide simple design
5. **Refactor regularly**: Keep code clean and aligned with these principles through continuous improvement
6. **Document architectural decisions**: Explain why you chose simplicity over more complex alternatives

### Common Anti-Patterns to Avoid

- **God classes**: Classes that do too much (violates SRP)
- **Premature generalization**: Creating abstract solutions before you understand the problem space (violates YAGNI)
- **Feature creep**: Adding features "while we're at it" (violates YAGNI)
- **Clever code**: Writing code that shows off programming skills but is hard to understand (violates KISS)
- **Tight coupling**: Classes that know too much about each other's implementation details (violates DIP)
