use crate::{SourceError, SourceErrorKind};

#[derive(Debug, Clone, Copy)]
pub struct BeforeIdCursor {
    prefix: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct OpaqueCursor {
    maximum_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct PageCursor {
    maximum: u32,
}

impl PageCursor {
    pub const fn new(maximum: u32) -> Self {
        Self { maximum }
    }

    pub fn encode(&self, page: u32) -> Result<String, SourceError> {
        if page > self.maximum {
            return Err(invalid_cursor());
        }
        Ok(page.to_string())
    }

    pub fn validate(&self, cursor: &str) -> Result<u32, SourceError> {
        let page = cursor.parse::<u32>().map_err(|_| invalid_cursor())?;
        (page <= self.maximum)
            .then_some(page)
            .ok_or_else(invalid_cursor)
    }
}

impl OpaqueCursor {
    pub const fn new(maximum_bytes: usize) -> Self {
        Self { maximum_bytes }
    }

    pub fn validate<'a>(&self, cursor: &'a str) -> Result<&'a str, SourceError> {
        if cursor.is_empty()
            || cursor.len() > self.maximum_bytes
            || cursor.chars().any(char::is_control)
        {
            return Err(invalid_cursor());
        }
        Ok(cursor)
    }
}

impl BeforeIdCursor {
    pub const fn new(prefix: &'static str) -> Self {
        Self { prefix }
    }

    pub fn encode(&self, id: impl std::fmt::Display) -> String {
        format!("{}{id}", self.prefix)
    }

    pub fn validate<'a>(&self, cursor: &'a str) -> Result<&'a str, SourceError> {
        let Some(id) = cursor.strip_prefix(self.prefix) else {
            return Err(invalid_cursor());
        };
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_cursor());
        }
        Ok(id)
    }
}

fn invalid_cursor() -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidQuery,
        "invalid source cursor",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_encodes_numeric_keyset_cursors() {
        let codec = BeforeIdCursor::new("b");
        assert_eq!(codec.encode(42), "b42");
        assert_eq!(codec.validate("b42").unwrap(), "42");
        assert!(codec.validate("42").is_err());
        assert!(codec.validate("bnope").is_err());
    }

    #[test]
    fn opaque_cursors_are_bounded_and_cannot_contain_controls() {
        let codec = OpaqueCursor::new(16);
        assert_eq!(codec.validate("next_abc-123").unwrap(), "next_abc-123");
        assert!(codec.validate("").is_err());
        assert!(codec.validate("line\nbreak").is_err());
        assert!(codec.validate("this-cursor-is-too-long").is_err());
    }

    #[test]
    fn page_cursors_are_bounded_decimal_values() {
        let codec = PageCursor::new(10_000);
        assert_eq!(codec.validate("42").unwrap(), 42);
        assert_eq!(codec.encode(42).unwrap(), "42");
        assert!(codec.validate("-1").is_err());
        assert!(codec.validate("10001").is_err());
    }
}
