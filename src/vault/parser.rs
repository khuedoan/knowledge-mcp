//! Markdown parsing with wiki-link extraction.
//!
//! This module parses markdown content using pulldown-cmark with wiki-link
//! support, extracting links, headings, and document structure.

use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use super::note::{Heading, WikiLink};

/// Parser options for pulldown-cmark with wiki-link support.
pub fn parser_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_WIKILINKS);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts
}

/// Result of parsing a markdown document.
#[derive(Debug, Default)]
pub struct ParseResult {
    /// Wiki links found in the document.
    pub links: Vec<WikiLink>,
    /// Headings found in the document.
    pub headings: Vec<Heading>,
    /// The first H1 heading (used as title).
    pub title: Option<String>,
}

/// Parse a markdown document and extract wiki links and headings.
pub fn parse_markdown(content: &str) -> ParseResult {
    let parser = Parser::new_ext(content, parser_options());

    let mut result = ParseResult::default();
    let mut current_heading_level: Option<u8> = None;
    let mut current_heading_text = String::new();
    let mut in_wiki_link: Option<(String, bool)> = None; // (dest_url, has_pothole)
    let mut wiki_link_text = String::new();

    for event in parser {
        match event {
            // Handle wiki links
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                title: _,
                id: _,
            }) => {
                in_wiki_link = Some((dest_url.to_string(), has_pothole));
                wiki_link_text.clear();
            }

            // Capture text inside wiki link (this is the alias if has_pothole is true)
            Event::Text(text) if in_wiki_link.is_some() && current_heading_level.is_none() => {
                wiki_link_text.push_str(&text);
            }

            // End of wiki link
            Event::End(TagEnd::Link) if in_wiki_link.is_some() => {
                if let Some((dest_url, has_pothole)) = in_wiki_link.take() {
                    let link = parse_wiki_link_with_text(&dest_url, has_pothole, &wiki_link_text);
                    result.links.push(link);
                }
            }

            // Handle headings
            Event::Start(Tag::Heading { level, .. }) => {
                current_heading_level = Some(heading_level_to_u8(level));
                current_heading_text.clear();
            }
            Event::Text(text) if current_heading_level.is_some() => {
                current_heading_text.push_str(&text);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_heading_level.take() {
                    let text = std::mem::take(&mut current_heading_text).trim().to_string();
                    if !text.is_empty() {
                        // Set title from first H1
                        if level == 1 && result.title.is_none() {
                            result.title = Some(text.clone());
                        }
                        result.headings.push(Heading::new(level, text));
                    }
                }
            }

            _ => {}
        }
    }

    result
}

/// Parse a wiki link from dest_url and text content.
///
/// With pulldown-cmark's wiki link extension:
/// - For `[[Note]]`, dest_url = "Note", has_pothole = false, text = "Note"
/// - For `[[Note|alias]]`, dest_url = "Note", has_pothole = true, text = "alias"  
/// - For `[[Note#heading]]`, dest_url = "Note#heading", has_pothole = false
fn parse_wiki_link_with_text(dest: &str, has_pothole: bool, text: &str) -> WikiLink {
    let dest = dest.trim();

    // Check for heading reference in destination
    let (base, heading) = if let Some(hash_pos) = dest.find('#') {
        let (b, h) = dest.split_at(hash_pos);
        (b, Some(h[1..].to_string())) // Skip the '#'
    } else {
        (dest, None)
    };

    // If has_pothole is true, the text is the display alias
    let display = if has_pothole && !text.is_empty() && text != base {
        Some(text.trim().to_string())
    } else {
        None
    };

    WikiLink {
        target: base.to_string(),
        display,
        heading,
    }
}

/// Parse a wiki link destination into a WikiLink struct (legacy, without text).
#[allow(dead_code)]
fn parse_wiki_link(dest: &str) -> WikiLink {
    parse_wiki_link_with_text(dest, false, "")
}

/// Convert pulldown-cmark HeadingLevel to u8.
fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_wiki_link() {
        let content = "Check out [[Deep Work]] for more info.";
        let result = parse_markdown(content);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "Deep Work");
        assert!(result.links[0].display.is_none());
        assert!(result.links[0].heading.is_none());
    }

    #[test]
    fn test_parse_wiki_link_with_alias() {
        let content = "See [[Deep Work|Cal Newport's book]] here.";
        let result = parse_markdown(content);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "Deep Work");
        assert_eq!(
            result.links[0].display,
            Some("Cal Newport's book".to_string())
        );
    }

    #[test]
    fn test_parse_wiki_link_with_heading() {
        let content = "Check [[Note#Section One]] for details.";
        let result = parse_markdown(content);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "Note");
        assert_eq!(result.links[0].heading, Some("Section One".to_string()));
    }

    #[test]
    fn test_parse_multiple_links() {
        let content = r#"
# My Note

This references [[Note A]] and [[Note B]].
Also see [[Note C|alias]].
"#;
        let result = parse_markdown(content);

        assert_eq!(result.links.len(), 3);
        assert_eq!(result.links[0].target, "Note A");
        assert_eq!(result.links[1].target, "Note B");
        assert_eq!(result.links[2].target, "Note C");
        assert_eq!(result.links[2].display, Some("alias".to_string()));
    }

    #[test]
    fn test_parse_headings() {
        let content = r#"
# Main Title

Some text.

## Section One

More text.

### Subsection

Even more text.

## Section Two
"#;
        let result = parse_markdown(content);

        assert_eq!(result.title, Some("Main Title".to_string()));
        assert_eq!(result.headings.len(), 4);
        assert_eq!(result.headings[0], Heading::new(1, "Main Title"));
        assert_eq!(result.headings[1], Heading::new(2, "Section One"));
        assert_eq!(result.headings[2], Heading::new(3, "Subsection"));
        assert_eq!(result.headings[3], Heading::new(2, "Section Two"));
    }

    #[test]
    fn test_parse_no_title_if_no_h1() {
        let content = r#"
## Not a title

Some content with [[Link]].
"#;
        let result = parse_markdown(content);

        assert!(result.title.is_none());
        assert_eq!(result.headings.len(), 1);
        assert_eq!(result.headings[0].level, 2);
    }

    #[test]
    fn test_parse_zettelkasten_example() {
        let content = r#"
# Zettelkasten Method

The Zettelkasten is a knowledge management method.

## Benefits

- Encourages [[Deep Work]] and focused thinking
- Builds a personal [[Knowledge Graph]]
- Supports [[Spaced Repetition]] through review

## Related Concepts

- [[Plain Text Files]] - The preferred format
- [[Linked Data]] - The web of connections
"#;
        let result = parse_markdown(content);

        assert_eq!(result.title, Some("Zettelkasten Method".to_string()));
        assert_eq!(result.links.len(), 5);

        let targets: Vec<&str> = result.links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"Deep Work"));
        assert!(targets.contains(&"Knowledge Graph"));
        assert!(targets.contains(&"Spaced Repetition"));
        assert!(targets.contains(&"Plain Text Files"));
        assert!(targets.contains(&"Linked Data"));
    }

    #[test]
    fn test_parse_empty_content() {
        let result = parse_markdown("");

        assert!(result.links.is_empty());
        assert!(result.headings.is_empty());
        assert!(result.title.is_none());
    }

    #[test]
    fn test_parse_content_without_links() {
        let content = r#"
# Just a Note

This note has no wiki links, just plain text.
And some **bold** and *italic* formatting.
"#;
        let result = parse_markdown(content);

        assert!(result.links.is_empty());
        assert_eq!(result.title, Some("Just a Note".to_string()));
        assert_eq!(result.headings.len(), 1);
    }

    #[test]
    fn test_parse_same_file_heading_link() {
        let content = "See [[#Local Section]] for more.";
        let result = parse_markdown(content);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "");
        assert_eq!(result.links[0].heading, Some("Local Section".to_string()));
    }
}
