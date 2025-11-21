# Contributing to Mujina Miner

Thank you for your interest in contributing to mujina-miner! We're excited to
have you here. This document will help you understand how to contribute
effectively and get your changes merged quickly.

The workflow described here is designed to make contributing easier---not
harder. By discussing changes before implementation, you'll get early
feedback, avoid wasted effort, and increase the chances your contribution
will be merged.

## Quick Guide

### I have a bug or something isn't working

1. Search [discussions] and the [issue tracker] for similar problems. Tip:
   also search [closed issues] and [closed discussions]---your issue might
   have already been fixed!
2. If your issue hasn't been reported, open an ["Issue Triage" discussion]
   and fill in the template completely. Use this category only for bug
   reports---thank you!

[discussions]: https://github.com/256foundation/mujina/discussions
[issue tracker]: https://github.com/256foundation/mujina/issues
[closed issues]: https://github.com/256foundation/mujina/issues?q=is%3Aissue+state%3Aclosed
[closed discussions]: https://github.com/256foundation/mujina/discussions?discussions_q=is%3Aclosed
["Issue Triage" discussion]: https://github.com/256foundation/mujina/discussions/new?category=issue-triage

### I have an idea for a feature

Open a discussion in the ["Ideas" category] to propose and discuss the
feature before implementation.

["Ideas" category]: https://github.com/256foundation/mujina/discussions/new?category=ideas

### I've implemented a feature

1. If there's an issue for the feature, open a pull request.
2. If there's no issue, open a discussion first and link to your branch.
   Getting feedback before the PR makes the review process smoother.

### I'd like to contribute

All issues are actionable---pick one and start working on it. Thank you! If
you need help or guidance, comment on the issue. Issues tagged with "good
first issue" are extra friendly to new contributors.

### I have a question

Open a [Q&A discussion] or ask in our community chat (if available).

[Q&A discussion]: https://github.com/256foundation/mujina/discussions/new?category=q-a

## General Patterns

### Issues are Actionable

**This workflow is adapted from [Ghostty]**. Thanks to Mitchell Hashimoto
and contributors for the pattern!

[Ghostty]: https://github.com/ghostty-org/ghostty

The mujina-miner [issue tracker] is for _actionable items_.

Unlike some projects, mujina-miner **does not use the issue tracker for
discussion or feature requests**. Instead, we use GitHub [discussions] for
that. Once a discussion reaches a point where a well-understood, actionable
item is identified, it's moved to the issue tracker. **This pattern makes it
easier for maintainers or contributors to find issues to work on since
_every issue_ is ready to be worked on.**

If you're experiencing a bug and have clear steps to reproduce it, open an
issue triage discussion. If you're experiencing a bug but aren't sure how to
reproduce it or aren't sure if it's a bug, open a discussion. If you have an
idea for a feature, open a discussion.

### Pull Requests Implement an Issue

Pull requests should be associated with a previously accepted issue. This
helps ensure your effort aligns with project goals and increases the
likelihood of a quick merge.

**Why this matters:** Without prior discussion, you risk spending time on
work that conflicts with planned changes, doesn't fit the project direction,
or solves a problem in a way that won't be merged. A quick discussion up
front saves everyone time.

**Exception:** Trivial improvements don't need discussion or an issue. This
includes typo fixes, wording improvements, spelling corrections, log level
adjustments, comment improvements, and documentation clarifications. Use
your judgment---if the change is obviously correct and self-contained, just
open the PR.

Issues tagged with "feature" or "enhancement" represent accepted,
well-scoped work. If you implement an issue as described, your pull request
will be accepted with a high degree of certainty.

> [!NOTE]
> **Design discussions belong in Discussions, not PRs.** If you want
> feedback on an approach before committing to the full implementation, open
> a discussion and link to your branch.

## Getting Started

### Prerequisites

- Rust toolchain (stable)
- Linux development environment
- Git
- Optional: Hardware for testing (Bitaxe boards, etc.)

### Setting Up Your Development Environment

1. Fork the repository on GitHub
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/mujina.git
   cd mujina-miner
   ```
3. Add the upstream repository:
   ```bash
   git remote add upstream https://github.com/256foundation/mujina.git
   ```
4. **Set up Git hooks** (required):
   ```bash
   ./scripts/setup-hooks.sh
   ```
   This configures automatic checks for whitespace errors and other issues.
5. Create a feature branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Process

### Before You Start

1. Check existing discussions and issues to avoid duplicate work
2. For significant changes, open a discussion first to get feedback on the
   approach before implementation
3. Read the architecture documentation in `docs/architecture.md`
4. Familiarize yourself with `CODE_STYLE.md` and `CODING_GUIDELINES.md`

### Making Changes

1. Write clean, idiomatic Rust code
2. Follow the project's module structure
3. Add tests for new functionality
4. Update documentation as needed
5. Ensure all tests pass: `cargo test`
6. Run clippy: `cargo clippy -- -D warnings`
7. Format your code: `cargo fmt`

### Testing

- **Unit tests**: Required for all new functionality
- **Integration tests**: For cross-module functionality
- **Hardware tests**: Mark with `#[ignore]` and document requirements
- **Protocol tests**: Use captured data when possible

Example test structure:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_functionality() {
        // Your test here
    }

    #[test]
    #[ignore] // Requires hardware
    fn test_with_hardware() {
        // Hardware-dependent test
    }
}
```

### Documentation

- Add doc comments to all public items
- Update module-level documentation
- Include examples in doc comments where helpful
- Keep markdown files wrapped at 79 characters
- Update architecture docs for significant changes
- Use asciiflow.com for creating ASCII diagrams in documentation

### Commit Messages

Write proper commit messages following these guidelines (adapted from the
Linux kernel contribution standards):

#### The Seven Rules of a Great Commit Message

1. **Separate subject from body with a blank line**
2. **Limit the subject line to 50 characters**
3. **Capitalize the subject line**
4. **Do not end the subject line with a period**
5. **Use the imperative mood in the subject line**
6. **Wrap the body at 72 characters**
7. **Use the body to explain what and why vs. how**

#### Format

```
type(scope): Subject in imperative mood

Longer explanation of what this commit does and why it was necessary.
The body should provide context for the change and explain what problem
it solves.

Wrap the body at 72 characters. Use the body to explain what changed
and why, not how (the code shows how).

Further paragraphs come after blank lines.

- Bullet points are okay too
- Use a hyphen or asterisk for bullets

Fixes: #123
Closes: #456
See-also: #789
```

#### Write Atomic Commits

Each commit should be a single logical change. Don't make several logical
changes in one commit. For example, if a patch fixes a bug and optimizes
the performance of a feature, split it into two separate commits.

**Good commit separation:**
- Commit 1: Fix buffer overflow in protocol parser
- Commit 2: Optimize protocol parser performance

**Bad commit (does too much):**
- Fix buffer overflow and optimize parser performance

#### Use Imperative Mood

Write your commit message as if you're giving orders to the codebase:
- GOOD: "Add temperature monitoring to board controller"
- GOOD: "Fix race condition in share submission"
- GOOD: "Refactor protocol handler to use async/await"
- BAD: "Added temperature monitoring"
- BAD: "Fixes race condition"
- BAD: "Refactoring protocol handler"

A properly formed Git commit subject should complete this sentence:
"If applied, this commit will _your subject here_"

#### Types
- `feat`: Add a new feature
- `fix`: Fix a bug
- `docs`: Change documentation only
- `style`: Change code style (formatting, missing semicolons, etc.)
- `refactor`: Refactor code without changing functionality
- `perf`: Improve performance
- `test`: Add or correct tests
- `chore`: Update build process, dependencies, etc.

#### Examples

**Good commit message:**
```
fix(board): prevent double-free in shutdown sequence

The board shutdown sequence could trigger a double-free when called
multiple times due to missing state check. This adds a proper state
machine to track shutdown progress and prevent multiple cleanup attempts.

The issue was discovered during stress testing with rapid board
connect/disconnect cycles.

Fixes: #234
```

**Another good example:**
```
feat(scheduler): implement work-stealing algorithm

Replace the simple round-robin scheduler with a work-stealing algorithm
that better balances load across multiple boards. Idle boards now steal
work from busy boards' queues.

Performance testing shows 15% improvement in share submission rate with
heterogeneous board configurations.
```

## Pull Request Process

**Before opening a PR:** Ensure there's an accepted issue for your change. If
not, open a discussion first to get feedback. (Exception: trivial improvements,
including typos, wording, log levels, comments, and documentation don't need an
issue. Use your judgment---if it's obviously correct and self-contained, just
open the PR.)

1. Update your branch with the latest upstream changes:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. Push your branch to your fork:
   ```bash
   git push origin feature/your-feature-name
   ```

3. Create a pull request on GitHub with:
   - Clear title describing the change
   - Description of what changed and why
   - **Reference to the issue being implemented** (required)
   - Link to the discussion that led to the issue (helpful context)
   - Screenshots/logs if applicable

4. Address review feedback promptly

5. Once approved, your PR will be merged

**Tip:** Following this process (discussion -> issue -> PR) dramatically
increases the chances of a quick merge. It shows you've done your homework
and the work aligns with project goals.

## Areas for Contribution

All issues in the [issue tracker] are actionable and ready to be worked on.
Pick one that matches your interests and skill level!

### Good First Issues

Look for issues labeled `good first issue` for beginner-friendly tasks:
- Documentation improvements
- Test coverage additions
- Small bug fixes
- Code cleanup

### High-Priority Areas

Check the issue tracker for issues labeled `priority` or `enhancement` in
these areas:
- Pool protocol implementations (Stratum v2)
- Additional ASIC chip support
- Hardware monitoring and safety features
- API endpoint implementations
- Performance optimizations

If you want to work on something not yet in the issue tracker, open a
discussion first to propose it.

## Hardware Testing

If you have mining hardware:
1. Test changes thoroughly before submitting
2. Document hardware-specific behavior
3. Provide debug logs with hardware interactions
4. Note any hardware limitations or quirks

## Questions and Support

See the [Quick Guide](#quick-guide) at the top of this document for routing
your question or request to the right place.

## Recognition

Contributors will be recognized in:
- The project's contributor list
- Release notes for significant contributions
- Special thanks for major features

Thank you for contributing to mujina-miner!
