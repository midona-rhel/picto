---
name: analyze
description: Conducts systematic technical analysis of issues, performance problems, architectural concerns, or improvement opportunities. Use for bug investigation, performance analysis, security assessment, or technical debt review.
argument-hint: [issue title] [description]
allowed-tools: Read, Grep, Glob, Write
context: fork
agent: Explore
---

# Technical Analysis

Conduct a comprehensive technical analysis for: $ARGUMENTS

## Analysis Methodology

1. **Problem Definition**: Clearly define the issue, performance concern, or improvement opportunity
2. **Investigation**: Use codebase exploration to gather evidence and understand the current state
3. **Root Cause Analysis**: Identify underlying causes and contributing factors
4. **Solution Options**: Develop multiple approaches with trade-off analysis
5. **Recommendations**: Provide specific, actionable recommendations with implementation guidance

## Investigation Areas

### Performance Analysis
- Identify bottlenecks and resource usage patterns
- Analyze database queries and operations
- Review async/await patterns and concurrency
- Examine memory usage and allocation patterns

### Security Assessment
- Input validation and sanitization
- Authentication and authorization flows
- Data encryption and storage security
- API security and rate limiting

### Architecture Review
- Code organization and modularity
- Design pattern compliance
- Dependency management
- Scalability considerations

### Bug Investigation
- Error reproduction and analysis
- Code flow tracing
- State management issues
- Integration point failures

## Deliverable: Analysis Report

Create a comprehensive analysis report in `docs/analysis/ANALYSIS-[date]-[issue-title].md` with:

1. **Executive Summary**
2. **Problem Statement**
3. **Investigation Findings** (with code examples and file references)
4. **Root Cause Analysis**
5. **Recommended Solutions** (prioritized with pros/cons)
6. **Implementation Plan**
7. **Success Metrics**

## Purpose

This skill helps Analysts perform comprehensive technical investigations, identify root causes, and provide actionable recommendations with supporting evidence.

## Analysis Types

- **Performance Analysis**: Bottleneck identification, optimization opportunities
- **Security Analysis**: Vulnerability assessment, security best practices review
- **Architecture Analysis**: Scalability assessment, design pattern evaluation
- **Bug Investigation**: Root cause analysis, fix recommendations
- **Code Quality Analysis**: Technical debt assessment, refactoring opportunities

## Features

- Systematic investigation methodology
- Evidence-based findings with code examples
- Multiple solution options with trade-off analysis
- Implementation guidance and success metrics
- Professional analysis report generation

## Usage

The skill takes an issue description or analysis request and produces a comprehensive technical analysis report.