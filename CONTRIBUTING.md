# Contributing to mempalace-rs

Thank you for your interest in contributing to mempalace-rs! This document provides guidelines and workflows for contributing.

## Development Setup

### Prerequisites

- Rust 1.70+ (with cargo)
- (Vector storage is fully embedded and zero-network)
- Optional: `cargo-llvm-cov` for coverage

### Building

```bash
git clone https://github.com/jxoesneon/mempalace-rs.git
cd mempalace-rs
cargo build
```

### Running Tests

```bash
# All tests
cargo test

# With coverage
cargo llvm-cov

# Specific module
cargo test dialect

# Test mode (skips DB init)
MEMPALACE_TEST_MODE=1 cargo test
```

## Code Standards

### Rust Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Run `cargo fmt` before committing
- Ensure `cargo clippy -- -D warnings` passes cleanly

### Testing

- Maintain 80%+ code coverage
- Write unit tests for pure functions
- Use integration tests for IO-heavy code
- Mock external dependencies when possible

### Documentation

- Document all public APIs with rustdoc
- Include examples in doc comments
- Update README.md for user-facing changes

## Pull Request Process

1. **Fork the repository** and create your branch from `main`
2. **Write tests** for new functionality
3. **Ensure all tests pass**: `cargo test`
4. **Check formatting**: `cargo fmt -- --check`
5. **Run linter**: `cargo clippy -- -D warnings`
6. **Update documentation** as needed
7. **Submit PR** with clear description of changes

## CI Checks

All PRs must pass:

- [ ] `cargo fmt -- --check`
- [ ] `cargo clippy -- -D warnings`
- [ ] `cargo build`
- [ ] `cargo test`
- [ ] Codecov coverage report

## Architecture Decisions

When making significant architectural changes:

1. Open an issue to discuss the proposal
2. Consider impact on the Memory Stack (L0-L3)
3. Ensure MCP tool compatibility
4. Update `mempalace-rs.skill` if MCP tools change

## Areas for Contribution

### Good First Issues

- Documentation improvements
- Additional test coverage
- CLI polish (progress bars, colorized output)

### Advanced Topics

- Wikipedia integration for entity type inference
- Spatial graph integration with Searcher ranking
- Additional MCP tools
- Performance optimizations

## Code of Conduct

Be respectful and constructive in all interactions.

## Questions?

- Open an issue for bugs or feature requests
- Check existing documentation and skill file

Thank you for contributing!
