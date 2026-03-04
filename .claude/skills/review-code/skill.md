---
name: review-code
description: Conducts comprehensive code reviews focusing on quality, security, performance, and adherence to project standards. Use when reviewing pull requests, code changes, or conducting security assessments.
argument-hint: [description] [file-paths] [pbi-number]
allowed-tools: Read, Grep, Glob, Write
---

# Code Review

Conduct a comprehensive code review for: $ARGUMENTS

## Review Process

1. **Read the specified files** and understand the changes
2. **Apply review checklists** based on technology and change type
3. **Identify issues** categorized by severity
4. **Provide constructive feedback** with specific examples and suggestions
5. **Generate review report** with actionable recommendations

## Review Checklists

### Rust/Tauri Backend Review
- [ ] **Memory Safety**: No unsafe blocks without justification, proper ownership patterns
- [ ] **Error Handling**: Result types used consistently, errors properly propagated
- [ ] **Async Patterns**: Proper async/await usage, no blocking in async contexts
- [ ] **Security**: Input validation, SQL injection prevention, command injection protection
- [ ] **Performance**: Efficient algorithms, minimal allocations, appropriate data structures
- [ ] **Testing**: Unit tests for business logic, integration tests for commands

### React/TypeScript Frontend Review
- [ ] **Component Design**: Single responsibility, proper prop typing, reusable components
- [ ] **State Management**: Appropriate state location, immutable updates, effect dependencies
- [ ] **TypeScript Usage**: Strong typing, no any types, proper interface definitions
- [ ] **Accessibility**: ARIA labels, keyboard navigation, semantic HTML
- [ ] **Performance**: Memoization where appropriate, lazy loading, efficient re-renders
- [ ] **Testing**: Component tests, user interaction tests, accessibility tests

### General Quality Review
- [ ] **Code Clarity**: Clear naming, appropriate comments, self-documenting code
- [ ] **Architecture**: Follows project patterns, appropriate abstractions, maintainable structure
- [ ] **Dependencies**: Necessary dependencies only, security vulnerabilities checked
- [ ] **Documentation**: Updated for significant changes, examples provided

## Issue Classification

### Critical Issues (Must Fix)
- Security vulnerabilities
- Memory safety violations
- Data corruption risks
- Breaking changes without migration

### Important Issues (Should Fix)
- Performance regressions
- Poor error handling
- Accessibility violations
- Inconsistent patterns

### Suggestions (Nice to Have)
- Code clarity improvements
- Performance optimizations
- Additional test coverage
- Documentation enhancements

## Deliverable: Review Report

Create a detailed review report in `docs/reviews/REVIEW-[date]-[description].md` with:

1. **Summary**: Overall assessment and recommendation (Approve/Approve with Changes/Reject)
2. **Strengths**: What was done well
3. **Issues Found**: Categorized by severity with file/line references
4. **Recommendations**: Specific actions to address issues
5. **Learning Opportunities**: Knowledge sharing and best practices

## Purpose

This skill helps Code Reviewers perform systematic code reviews with technology-specific checklists, security assessments, and constructive feedback.

## Review Areas

- **Code Quality**: Readability, maintainability, consistency with project patterns
- **Security**: Vulnerability identification, input validation, data protection
- **Performance**: Bottleneck identification, resource usage optimization
- **Architecture**: Compliance with design patterns and project structure
- **Testing**: Test coverage, quality, and maintainability

## Technology Focus

- **Rust Backend**: Memory safety, error handling, async patterns, database operations
- **React Frontend**: Component design, state management, TypeScript usage, accessibility
- **Tauri IPC**: Command security, type safety, error propagation

## Features

- Comprehensive review checklists
- Security-focused analysis
- Constructive feedback with examples
- Priority classification (Critical/Important/Suggestion)
- Learning opportunity identification

## Usage

The skill takes file paths or code changes and produces a detailed code review report with actionable feedback.