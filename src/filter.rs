//! Sensitive data detection for content filtering.
//!
//! This module provides keyword-based detection of potentially sensitive
//! content in notes, enabling user confirmation before exposing such data.

/// Result of checking content for sensitive data.
#[derive(Debug, Clone)]
pub struct SensitiveCheckResult {
    /// Whether sensitive data was detected.
    pub is_sensitive: bool,
    /// Keywords that were matched.
    pub matched_keywords: Vec<String>,
}

/// Filter for detecting sensitive data in content.
#[derive(Debug, Clone)]
pub struct SensitiveDataFilter {
    keywords: Vec<String>,
}

impl SensitiveDataFilter {
    /// Create a new filter with the given keywords.
    pub fn new(keywords: Vec<String>) -> Self {
        Self { keywords }
    }

    /// Check content for sensitive data.
    ///
    /// Performs case-insensitive keyword matching.
    pub fn check(&self, content: &str) -> SensitiveCheckResult {
        let content_lower = content.to_lowercase();
        let matched_keywords: Vec<String> = self
            .keywords
            .iter()
            .filter(|keyword| content_lower.contains(keyword.as_str()))
            .cloned()
            .collect();

        SensitiveCheckResult {
            is_sensitive: !matched_keywords.is_empty(),
            matched_keywords,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_sensitive_data() {
        let filter = SensitiveDataFilter::new(vec!["password".to_string(), "salary".to_string()]);
        let result = filter.check("This is a normal note about programming.");
        assert!(!result.is_sensitive);
        assert!(result.matched_keywords.is_empty());
    }

    #[test]
    fn test_sensitive_keyword_detected() {
        let filter = SensitiveDataFilter::new(vec!["password".to_string(), "salary".to_string()]);
        let result = filter.check("My salary is $100,000 per year.");
        assert!(result.is_sensitive);
        assert_eq!(result.matched_keywords, vec!["salary"]);
    }

    #[test]
    fn test_case_insensitive() {
        let filter = SensitiveDataFilter::new(vec!["password".to_string()]);
        let result = filter.check("My PASSWORD is secret123.");
        assert!(result.is_sensitive);
        assert_eq!(result.matched_keywords, vec!["password"]);
    }

    #[test]
    fn test_multiple_keywords() {
        let filter = SensitiveDataFilter::new(vec![
            "password".to_string(),
            "salary".to_string(),
            "ssn".to_string(),
        ]);
        let result = filter.check("Password: xyz, SSN: 123-45-6789");
        assert!(result.is_sensitive);
        assert!(result.matched_keywords.contains(&"password".to_string()));
        assert!(result.matched_keywords.contains(&"ssn".to_string()));
    }
}
