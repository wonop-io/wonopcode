//! Custom assertion helpers for common test patterns.
//!
//! Provides macros and functions for making test assertions more readable
//! and providing better error messages.

use std::path::Path;

/// Assert that a file contains specific text.
///
/// # Example
///
/// ```rust
/// use wonopcode_test_utils::assertions::assert_file_contains;
/// use std::fs;
/// use tempfile::TempDir;
///
/// let dir = TempDir::new().unwrap();
/// let path = dir.path().join("test.txt");
/// fs::write(&path, "Hello, world!").unwrap();
///
/// assert_file_contains(&path, "Hello");
/// assert_file_contains(&path, "world");
/// ```
pub fn assert_file_contains(path: &Path, expected: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path.display(), e));

    assert!(
        content.contains(expected),
        "File {} does not contain expected text.\nExpected to find: {}\nActual content:\n{}",
        path.display(),
        expected,
        content
    );
}

/// Assert that a file does not contain specific text.
pub fn assert_file_not_contains(path: &Path, unexpected: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path.display(), e));

    assert!(
        !content.contains(unexpected),
        "File {} unexpectedly contains: {}\nActual content:\n{}",
        path.display(),
        unexpected,
        content
    );
}

/// Assert that a file's content equals expected text exactly.
pub fn assert_file_equals(path: &Path, expected: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path.display(), e));

    assert_eq!(
        content,
        expected,
        "File {} content does not match expected.\nExpected:\n{}\nActual:\n{}",
        path.display(),
        expected,
        content
    );
}

/// Assert that two strings are equal, with a nice diff on failure.
pub fn assert_strings_equal(actual: &str, expected: &str) {
    if actual != expected {
        let diff = similar::TextDiff::from_lines(expected, actual);
        let mut output = String::new();

        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Delete => "-",
                similar::ChangeTag::Insert => "+",
                similar::ChangeTag::Equal => " ",
            };
            output.push_str(&format!("{}{}", sign, change));
        }

        panic!("Strings are not equal.\nDiff:\n{}", output);
    }
}

/// Assert that a result is Ok and extract the value.
#[macro_export]
macro_rules! assert_ok {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(e) => panic!("Expected Ok, got Err: {:?}", e),
        }
    };
    ($expr:expr, $msg:literal) => {
        match $expr {
            Ok(value) => value,
            Err(e) => panic!("{}: {:?}", $msg, e),
        }
    };
}

/// Assert that a result is Err.
#[macro_export]
macro_rules! assert_err {
    ($expr:expr) => {
        match $expr {
            Ok(value) => panic!("Expected Err, got Ok: {:?}", value),
            Err(e) => e,
        }
    };
    ($expr:expr, $msg:literal) => {
        match $expr {
            Ok(value) => panic!("{}: {:?}", $msg, value),
            Err(e) => e,
        }
    };
}

/// Assert that an option is Some and extract the value.
#[macro_export]
macro_rules! assert_some {
    ($expr:expr) => {
        match $expr {
            Some(value) => value,
            None => panic!("Expected Some, got None"),
        }
    };
    ($expr:expr, $msg:literal) => {
        match $expr {
            Some(value) => value,
            None => panic!("{}", $msg),
        }
    };
}

/// Assert that an option is None.
#[macro_export]
macro_rules! assert_none {
    ($expr:expr) => {
        if let Some(value) = $expr {
            panic!("Expected None, got Some: {:?}", value);
        }
    };
    ($expr:expr, $msg:literal) => {
        if let Some(value) = $expr {
            panic!("{}: {:?}", $msg, value);
        }
    };
}

/// Assert that a collection contains an item.
#[macro_export]
macro_rules! assert_contains {
    ($collection:expr, $item:expr) => {
        if !$collection.iter().any(|x| x == &$item) {
            panic!(
                "Collection does not contain expected item.\nExpected: {:?}\nCollection: {:?}",
                $item, $collection
            );
        }
    };
}

/// Assert that a string contains a substring (with better error messages).
#[macro_export]
macro_rules! assert_str_contains {
    ($haystack:expr, $needle:expr) => {
        if !$haystack.contains($needle) {
            panic!(
                "String does not contain expected substring.\nExpected to find: {}\nIn string:\n{}",
                $needle, $haystack
            );
        }
    };
}

/// Assert that a duration is within a range.
pub fn assert_duration_within(
    actual: std::time::Duration,
    min: std::time::Duration,
    max: std::time::Duration,
) {
    assert!(
        actual >= min && actual <= max,
        "Duration {:?} is not within range [{:?}, {:?}]",
        actual,
        min,
        max
    );
}

/// Assert that a value is approximately equal (for floating point comparisons).
pub fn assert_approx_eq(actual: f64, expected: f64, epsilon: f64) {
    let diff = (actual - expected).abs();
    assert!(
        diff < epsilon,
        "Values are not approximately equal.\nActual: {}\nExpected: {}\nDifference: {} (epsilon: {})",
        actual,
        expected,
        diff,
        epsilon
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_assert_file_contains() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "Hello, world!").unwrap();

        assert_file_contains(&path, "Hello");
        assert_file_contains(&path, "world");
    }

    #[test]
    fn test_assert_file_not_contains() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "Hello, world!").unwrap();

        assert_file_not_contains(&path, "goodbye");
    }

    #[test]
    fn test_assert_strings_equal() {
        assert_strings_equal("hello", "hello");
    }

    #[test]
    fn test_assert_ok_macro() {
        let result: Result<i32, &str> = Ok(42);
        let value = assert_ok!(result);
        assert_eq!(value, 42);
    }

    #[test]
    fn test_assert_some_macro() {
        let option: Option<i32> = Some(42);
        let value = assert_some!(option);
        assert_eq!(value, 42);
    }

    #[test]
    fn test_assert_approx_eq() {
        assert_approx_eq(std::f64::consts::PI, std::f64::consts::PI + 0.00001, 0.001);
    }
}
