---
name: role-assign
description: Determines and assigns the appropriate role for a Claude agent based on task context and requirements. Use when you need to assign roles like Business Owner, Developer, Analyst, or Reviewer.
argument-hint: [task description]
---

# Role Assignment Skill

Analyze the task description in $ARGUMENTS and determine the most appropriate role for the Claude agent to take.

## Task Analysis Process

1. **Examine the task description** for keywords, scope, and technical requirements
2. **Identify the primary activity** (management, analysis, development, review)
3. **Consider the technical domain** (frontend, backend, UX, general)
4. **Assign the most appropriate role** and explain the reasoning

## Available Roles

### Management & Strategy
- **Business Owner** - Strategic oversight, requirements definition, project direction, stakeholder communication
- **PBI Manager** - Product Backlog Item creation, requirements analysis, task breakdown, specification writing

### Analysis & Review
- **Analyst** - Technical analysis, issue investigation, improvement recommendations, root cause analysis
- **Code Reviewer** - Code quality assessment, security review, best practices enforcement, pull request review

### Development Specializations
- **Backend Developer** - Rust/Tauri backend implementation, database operations, API development, system integration
- **Frontend Developer** - React/TypeScript UI implementation, component development, state management, user interactions
- **UX Developer** - User experience design, accessibility, interaction patterns, usability testing, design systems

### General Implementation
- **Worker** - General development tasks, bug fixes, documentation, testing, maintenance

## Assignment Output

For the task: "$ARGUMENTS"

**Assigned Role**: [Role Name]

**Reasoning**: [Why this role is most appropriate based on task requirements and scope]

**Next Steps**: [Recommended actions for someone in this role to take]