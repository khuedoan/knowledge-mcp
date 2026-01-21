# Unix Philosophy

The Unix Philosophy is a set of cultural norms and philosophical approaches to minimalist, modular software development.

## Core Tenets

As summarized by Doug McIlroy:

1. **Do one thing well** - Write programs that do one thing and do it well
2. **Work together** - Write programs to work together
3. **Text streams** - Write programs to handle text streams, the universal interface

## Extended Principles

Peter H. Salus summarized:

- Write programs that do one thing and do it well
- Write programs to work together
- Write programs to handle [[Plain Text Files]], because that is a universal interface

## KISS Principle

"Keep It Simple, Stupid" - Complexity is the enemy:
- Simple code is easier to debug
- Simple interfaces are easier to use
- Simple systems are easier to maintain

## Application Today

The Unix Philosophy influences:
- [[Programming Fundamentals]] - Function design
- [[Git Basics]] - Small, composable commands
- Microservices architecture
- Command-line tools

## Examples

```bash
# Composing simple tools
cat file.txt | grep "pattern" | sort | uniq -c

# Each tool does one thing, combined they're powerful
```

## Contrast with Monoliths

| Unix Way | Monolithic |
|----------|------------|
| Small tools | Large applications |
| Text interfaces | Binary formats |
| Composable | Self-contained |
| Replaceable parts | Tightly coupled |

## Influence on PKM

The [[Zettelkasten Method]] follows similar principles:
- Atomic notes (do one thing)
- [[Linked Data]] (work together)
- [[Plain Text Files]] (universal interface)

## See Also

- [[Computer Science Basics]]
- [[Deep Work]] - Focus on one thing
