# Git Basics

Git is a distributed version control system created by Linus Torvalds in 2005 for Linux kernel development.

## Core Concepts

### Repository

A repository (repo) contains all project files and their complete history.

### Commits

Snapshots of your project at a point in time:
```bash
git add file.txt
git commit -m "Add feature X"
```

### Branches

Parallel lines of development:
```bash
git branch feature-name
git checkout feature-name
# or combined:
git checkout -b feature-name
```

## Why Git?

1. **History** - Complete record of all changes
2. **Collaboration** - Multiple people can work together
3. **Branching** - Experiment without breaking main code
4. **Distributed** - Everyone has full copy

## Git for Notes

Version control works excellently for [[Plain Text Files]]:
- Track changes to your [[Zettelkasten Method]] over time
- Sync between devices
- Never lose work
- See how ideas evolved

## Essential Commands

| Command | Purpose |
|---------|---------|
| `git init` | Create new repo |
| `git clone` | Copy existing repo |
| `git status` | See current state |
| `git diff` | See changes |
| `git log` | View history |

## The DAG

Git's history forms a Directed Acyclic Graph - a concept from [[Graph Theory]]:
- Commits are nodes
- Parent references are edges
- Branches are pointers to commits

## Best Practices

- Commit often, push regularly
- Write meaningful commit messages
- Use branches for features
- Review before merging

## See Also

- [[Unix Philosophy]] - Git embodies these principles
- [[Programming Fundamentals]] - Essential developer skill
