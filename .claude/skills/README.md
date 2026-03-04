# Picto Project - Claude Agent Skills

This directory contains Claude skills for organizing and managing development work on the Picto project. These skills enable efficient role-based collaboration and streamlined development processes.

## Available Skills

### 🎯 `/role-assign` - Role Assignment
**Purpose**: Determines and assigns the appropriate role for a Claude agent based on task context.

**Available Roles**:
- **Business Owner** - Strategic oversight, requirements definition, project direction
- **PBI Manager** - Product Backlog Item creation, requirements analysis, task breakdown
- **Analyst** - Technical analysis, issue investigation, improvement recommendations
- **Code Reviewer** - Code quality assessment, security review, best practices enforcement
- **Backend Developer** - Rust/Tauri backend implementation, database operations, API development
- **Frontend Developer** - React/TypeScript UI implementation, component development
- **UX Developer** - User experience design, accessibility, interaction patterns
- **Worker** - General development tasks, bug fixes, documentation

**Usage**: `/role-assign 'Implement dark mode feature in the settings panel'`

### 📋 `/create-pbi` - PBI Creation
**Purpose**: Creates detailed Product Backlog Items from business requirements.

**Features**:
- Automatic PBI numbering (continues from PBI-057)
- Comprehensive template with acceptance criteria
- Technical specifications for Rust and React
- Testing and documentation requirements

**Usage**: `/create-pbi 'Dark Mode Toggle' 'Users need ability to switch to dark theme for better visibility'`

### 🔍 `/analyze` - Technical Analysis
**Purpose**: Conducts systematic technical analysis of issues and improvement opportunities.

**Analysis Types**:
- Performance Analysis
- Security Analysis
- Architecture Analysis
- Bug Investigation
- Code Quality Analysis

**Usage**: `/analyze 'Database Performance Issues' 'Application becomes unresponsive during large image imports'`

### 📖 `/review-code` - Code Review
**Purpose**: Conducts comprehensive code reviews with technology-specific checklists.

**Review Areas**:
- Security assessment
- Code quality and maintainability
- Performance optimization
- Architecture compliance
- Technology-specific best practices (Rust/React)

**Usage**: `/review-code 'Dark mode implementation' 'src/components/Settings.tsx,src-tauri/src/commands/ui_commands.rs' 'PBI-058'`

### 👨‍💻 `/assign-work` - Work Assignment
**Purpose**: Assigns implementation tasks to appropriate developer specializations.

**Assignment Types**:
- Backend Developer (Rust/Tauri)
- Frontend Developer (React/TypeScript)
- UX Developer (User Experience)
- General Worker (Cross-cutting concerns)

**Usage**: `/assign-work 'Implement dark mode toggle in settings' 'src/components/Settings.tsx,src-tauri/src/commands/ui_commands.rs' 'PBI-058'`

## Skill Workflows

### 1. Feature Development Workflow
```
Business Owner → /role-assign → Business Owner Role
Business Owner defines requirements
Business Owner → /create-pbi → PBI Created
PBI Manager → /assign-work → Developer Assigned
Developer implements feature
Code Reviewer → /review-code → Review Complete
```

### 2. Issue Investigation Workflow
```
Any Role → /role-assign → Analyst Role
Analyst → /analyze → Analysis Report
Business Owner approves solution
PBI Manager → /create-pbi → Implementation PBI
Developer → /assign-work → Work Assigned
```

### 3. Code Review Workflow
```
Developer completes implementation
Code Reviewer → /review-code → Review Report
If issues found → Developer fixes → Re-review
If approved → Merge to main branch
```

## Project Context

### Technology Stack
- **Backend**: Rust with Tauri framework
- **Frontend**: React 18 + TypeScript + Mantine UI
- **Database**: SQLite with chunked blob storage
- **Build**: Vite (frontend) + Cargo (backend)

### Current State
- **PBI Range**: PBI-001 through PBI-057 (auto-incremented)
- **Known Issues**: Documented in `docs/review/` directory
- **Architecture**: Monolithic database.rs (4100+ lines), performance bottlenecks

### Key Directories
- `src/` - React frontend components
- `src-tauri/` - Rust backend implementation
- `docs/pbi/` - Product Backlog Items
- `docs/review/` - Architectural analysis
- `docs/analysis/` - Technical analysis reports
- `docs/reviews/` - Code review reports
- `docs/assignments/` - Work assignments

## Getting Started

### For New Agents
1. Use `/role-assign '<your task description>'` to determine your role
2. Load the appropriate role-specific system prompt and guidelines
3. Begin work using your role's processes and responsibilities

### For Business Owners
- Use `/create-pbi` to convert requirements into implementation specifications
- Use `/assign-work` to assign tasks to appropriate developer specializations
- Review analysis reports and approve technical approaches

### For Developers
- Check `/assign-work` output for role-specific implementation guidance
- Use existing patterns in the codebase for consistency
- Submit work for `/review-code` before merging

### For Quality Assurance
- Use `/analyze` for systematic technical investigations
- Use `/review-code` for comprehensive code quality assessment
- Focus on security, performance, and maintainability

## Collaboration Guidelines

### Communication Standards
- Use skill-generated documentation for consistent formatting
- Reference PBI numbers in all related work
- Include file paths and line numbers in technical discussions
- Document decisions and rationale for future reference

### Quality Standards
- All code must pass review before merging
- Security considerations are mandatory for all changes
- Performance implications must be assessed
- Accessibility standards must be maintained

### Process Efficiency
- Use parallel work assignment when possible
- Minimize handoff delays between roles
- Maintain clear documentation throughout development
- Focus on business value and user impact

## Examples

### Example 1: New Feature Request
```bash
# Business Owner determines requirements
/role-assign "Add ability to export image collections as ZIP files"

# Create formal PBI
/create-pbi "Export Collections as ZIP" "Users want to share collections by exporting them as downloadable ZIP archives"

# Assign implementation work
/assign-work "Implement ZIP export functionality" "src/components/Collections.tsx,src-tauri/src/commands/export_commands.rs" "PBI-058"

# Review implementation when complete
/review-code "ZIP export feature implementation" "src/components/Collections.tsx,src-tauri/src/commands/export_commands.rs" "PBI-058"
```

### Example 2: Performance Issue Investigation
```bash
# Investigate performance problem
/analyze "Image Grid Performance" "Image grid becomes slow when displaying more than 1000 images"

# Create PBI for optimization work
/create-pbi "Image Grid Virtualization" "Implement virtual scrolling to handle large image collections efficiently"

# Assign to frontend specialist
/assign-work "Implement virtual scrolling in image grid" "src/components/ImageGrid.tsx" "PBI-059"
```

### Example 3: Security Review
```bash
# Conduct security analysis
/analyze "File Upload Security" "Review file upload mechanisms for potential security vulnerabilities"

# Review specific implementation
/review-code "File upload security review" "src-tauri/src/commands/file_commands.rs,src/components/FileUpload.tsx"
```

## Success Metrics
- **Development Velocity**: Features delivered per sprint
- **Quality**: Defects found in production vs. development
- **Collaboration**: Handoff efficiency between roles
- **User Satisfaction**: Business owner approval rate on delivered features

---

**Tip**: Each skill generates comprehensive documentation to ensure consistent, high-quality development processes across all team members.
