---
name: create-pbi
description: Creates a detailed Product Backlog Item (PBI) from business requirements, following the established project format and numbering system. Use when converting requirements into implementable specifications.
argument-hint: [title] [description]
allowed-tools: Read, Glob, Write
---

# Create PBI Skill

Create a comprehensive Product Backlog Item (PBI) from the requirements provided in $ARGUMENTS.

## Process

1. **Determine PBI Number**: Check existing PBIs in `docs/pbi/` to assign the next sequential number
2. **Analyze Requirements**: Break down the requirements into technical specifications
3. **Create PBI Document**: Generate a detailed PBI following the established template
4. **Save to File**: Write the PBI to `docs/pbi/PBI-XXX-[kebab-case-title].md`

## PBI Template Structure

Use this template for all PBIs:

```markdown
# PBI-XXX: [Title]

## Summary
[Brief description of the feature/requirement]

## Business Value
[Why this is important and what value it provides]

## Acceptance Criteria
- [ ] [Specific, testable criteria]
- [ ] [Another criteria]
- [ ] [Final criteria]

## Technical Specifications

### Frontend (React/TypeScript)
- Components to create/modify: [list]
- State management requirements: [details]
- UI/UX considerations: [notes]

### Backend (Rust/Tauri)
- Commands to create/modify: [list]
- Database changes: [schema updates]
- API endpoints: [new/modified endpoints]

## Implementation Notes
[Technical considerations, dependencies, potential challenges]

## Testing Requirements
- Unit tests: [areas to test]
- Integration tests: [workflows to verify]
- Manual testing: [user scenarios]

## Definition of Done
- [ ] Code implemented and reviewed
- [ ] Tests written and passing
- [ ] Documentation updated
- [ ] Feature manually verified
```

## Task
Create a PBI for: $ARGUMENTS

1. Assign the next available PBI number
2. Use the template above
3. Fill in all sections based on the requirements
4. Save to the appropriate file in `docs/pbi/`