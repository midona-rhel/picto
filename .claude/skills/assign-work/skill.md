---
name: assign-work
description: Assigns development work to the appropriate specialized developer role (Backend, Frontend, UX Developer, or General Worker) based on task requirements and codebase impact. Use when delegating implementation tasks.
argument-hint: [task description] [file-paths] [pbi-number]
allowed-tools: Read, Grep, Glob, Write
---

# Work Assignment

Analyze the task and assign to the appropriate developer specialization: $ARGUMENTS

## Assignment Analysis Process

1. **Parse Task Requirements**: Understand scope, complexity, and technical domains involved
2. **Analyze Affected Files**: Examine file paths and technology stack areas
3. **Determine Primary Domain**: Identify whether work is primarily backend, frontend, UX, or cross-cutting
4. **Assess Complexity**: Consider whether task needs specialized expertise or can be general work
5. **Make Assignment**: Choose the most appropriate developer type with detailed reasoning

## Developer Specializations

### Backend Developer (Rust/Tauri)
**Assign when task involves:**
- Rust code in `src-tauri/` directory
- Database operations, migrations, or schema changes
- Tauri commands and IPC communication
- System integrations and external API calls
- Performance optimization of backend operations
- Security implementations (encryption, authentication)

### Frontend Developer (React/TypeScript)
**Assign when task involves:**
- React components in `src/` directory
- TypeScript interfaces and type definitions
- State management (hooks, context, external libraries)
- UI component libraries (Mantine, etc.)
- Frontend build system and bundling
- Client-side performance optimization

### UX Developer (User Experience)
**Assign when task involves:**
- User interface design and layout
- Accessibility improvements (ARIA, keyboard navigation)
- User interaction patterns and workflows
- Responsive design and mobile optimization
- Usability testing and user feedback implementation
- Design system components and consistency

### General Worker (Cross-cutting)
**Assign when task involves:**
- Documentation updates
- Testing infrastructure and test writing
- Build system configuration
- DevOps and deployment processes
- Bug fixes spanning multiple domains
- Simple maintenance tasks

## Assignment Output Format

For task: "$ARGUMENTS"

**Assigned Role**: [Developer Specialization]

**Primary Justification**: [Why this role is most appropriate]

**Technical Scope**:
- Primary domain: [Backend/Frontend/UX/Cross-cutting]
- Complexity level: [Simple/Moderate/Complex]
- Estimated effort: [Small/Medium/Large]

**Specific Guidance for Assigned Developer**:
- Key files to focus on: [list]
- Technical considerations: [important points]
- Dependencies and prerequisites: [if any]
- Success criteria: [what "done" looks like]

**Dependencies**: [Other team members or systems that may be involved]

**Next Steps**: [Recommended immediate actions for the assigned developer]

## Purpose

This skill helps Business Owners and PBI Managers efficiently assign implementation tasks to developers with the right expertise for optimal productivity and quality.

## Assignment Types

- **Backend Developer**: Rust/Tauri backend, database operations, API development, system integration
- **Frontend Developer**: React/TypeScript UI, component development, state management, user interactions
- **UX Developer**: User experience design, accessibility, interaction patterns, usability optimization
- **General Worker**: Cross-cutting concerns, documentation, testing, maintenance tasks

## Features

- Analyzes task requirements and technical scope
- Considers affected files and technology areas
- Provides role-specific guidance and context
- Includes implementation priorities and dependencies
- Generates work assignment documentation

## Usage

The skill takes a PBI or task description and determines the most appropriate developer specialization for implementation.