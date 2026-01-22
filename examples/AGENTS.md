# Writing Guidelines

This document describes the conventions for creating and maintaining notes in this knowledge vault.

## Zettelkasten Principles

1. **Atomicity**: Each note should contain ONE idea. If you're explaining multiple concepts, create multiple notes.
2. **Self-contained**: A note should be understandable on its own, without requiring other notes to make sense.
3. **Connectivity**: Always look for connections to existing notes using [[Wiki Links]].
4. **Unique Titles**: Each note has a unique, descriptive title in Title Case.

## Note Structure

### Title

- Use H1 (`#`) for the title
- Title should match the note name
- Use noun phrases, not sentences (e.g., "Deep Work" not "How Deep Work Helps You Focus")

### Content

- Start with a concise definition or explanation (1-2 sentences)
- Use H2 (`##`) for major sections
- Keep paragraphs short (3-5 sentences max)
- Use bullet points for lists of characteristics/features

### Links

- Link to related concepts inline: `This relates to [[Other Concept]]`
- Add a `## See Also` or `## Related Concepts` section at the end for additional connections
- Before creating a new note, search for existing notes that might already cover the topic

### References

When citing sources, add a `## References` section at the bottom:

```markdown
## References

- [Source Title](URL) - Accessed YYYY-MM-DD
  - *Optional insight about why this source was relevant*
```

## Markdown Conventions

- Use **bold** for key terms on first mention
- Use `code` for technical terms, commands, file names
- Use > blockquotes for direct quotes
- Use tables for comparisons

## Example Note

```markdown
# Spaced Repetition

Spaced repetition is a learning technique that involves reviewing information at increasing intervals to optimize long-term retention.

## How It Works

The core principle is based on the **forgetting curve** - we forget information at a predictable rate. By reviewing just before we forget, we strengthen the memory with minimal effort.

## Benefits

- More efficient than massed practice (cramming)
- Builds durable long-term memory
- Works for any memorization task

## Applications

- Learning vocabulary for [[Language Learning]]
- Studying for exams
- Memorizing [[Programming Fundamentals]]

## See Also

- [[Deep Work]] - Deep focus enhances retention
- [[Personal Knowledge Management]]

## References

- [Gwern on Spaced Repetition](https://www.gwern.net/Spaced-repetition) - Accessed 2026-01-22
  - *Comprehensive overview with research citations*
```
