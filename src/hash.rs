use crate::types::ID_PREFIX;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Base36 alphabet for encoding (0-9, a-z)
const BASE36_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Encode bytes to base36 string of specified length
///
/// Converts a byte slice to a base36 string representation.
/// Uses big-integer arithmetic to convert bytes to base36.
/// Pads with leading zeros if needed to reach the target length.
///
/// # Arguments
/// * `data` - Byte slice to encode
/// * `length` - Desired output length in base36 characters
///
/// # Returns
/// A base36 string of the specified length
fn encode_base36(data: &[u8], length: usize) -> String {
    use num_bigint::BigUint;
    use num_traits::Zero;

    // Convert bytes to big integer
    let mut num = BigUint::from_bytes_be(data);

    // Handle zero case
    let zero = BigUint::zero();
    if num == zero {
        return "0".repeat(length);
    }

    // Convert to base36
    let base = BigUint::from(36u32);
    let mut chars = Vec::new();

    // Build the string in reverse
    while num > zero {
        let remainder = &num % &base;
        num /= &base;
        // remainder is always 0-35, so this is safe
        let digit_idx = if let Some(digits) = remainder.to_u32_digits().first() {
            *digits as usize
        } else {
            0
        };
        chars.push(BASE36_ALPHABET[digit_idx]);
    }

    // Reverse to get correct order
    chars.reverse();

    // Convert to string
    let mut result = String::from_utf8(chars).unwrap_or_else(|_| String::from("0"));

    // Pad with zeros if needed
    if result.len() < length {
        result = "0".repeat(length - result.len()) + &result;
    }

    // Truncate to exact length if needed (keep least significant digits)
    if result.len() > length {
        result = result[result.len() - length..].to_string();
    }

    result
}

/// True if the part of `id` after the last `-` consists only of ASCII digits.
fn is_all_digit_suffix(id: &str) -> bool {
    let suffix = id.rsplit('-').next().unwrap_or(id);
    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
}

/// Generate a hash-based ID with collision handling
///
/// Takes a collision checker function that returns true if the ID already exists.
/// Uses adaptive length based on estimated database size, and tries multiple nonces
/// before escalating to longer hash lengths.
///
/// # Arguments
/// * `title` - Issue title
/// * `description` - Issue description
/// * `timestamp` - Creation timestamp for deterministic hashing
/// * `estimated_db_size` - Approximate number of existing issues (for adaptive length)
/// * `collision_check` - Function that returns true if an ID already exists
///
/// # Returns
/// A unique hash-based ID like "tazk-4f1" or "tazk-b127a5" (the hash always
/// contains at least one letter)
pub fn generate_hash_id_with_collision_check<F>(
    title: &str,
    description: &str,
    timestamp: DateTime<Utc>,
    estimated_db_size: usize,
    mut collision_check: F,
) -> anyhow::Result<String>
where
    F: FnMut(&str) -> bool,
{
    let creator = "user"; // Default creator

    // Adaptive length based on database size
    let initial_length = if estimated_db_size < 10 {
        3
    } else if estimated_db_size < 100 {
        4
    } else if estimated_db_size < 1000 {
        5
    } else if estimated_db_size < 10000 {
        6
    } else if estimated_db_size < 100000 {
        7
    } else {
        8
    };

    // Try adaptive lengths starting from initial_length, checking for collisions
    for length in initial_length..=8 {
        for nonce in 0..10 {
            let candidate = generate_hash_id(title, description, creator, timestamp, length, nonce);

            // An all-digit suffix would be indistinguishable from a numeric ID,
            // so treat it like a collision and move on to the next nonce/length.
            if is_all_digit_suffix(&candidate) {
                continue;
            }

            // Check for collision using provided function
            if !collision_check(&candidate) {
                return Ok(candidate);
            }
        }
    }

    anyhow::bail!(
        "Failed to generate unique hash ID after trying all lengths and nonces (database has ~{} issues)",
        estimated_db_size
    )
}

/// Generate a hash-based issue ID from content and metadata.
///
/// The hash is deterministic based on title, description, creator, timestamp, and nonce.
/// The hash is base36 encoded.
///
/// # Arguments
/// * `title` - Issue title
/// * `description` - Issue description
/// * `creator` - Issue creator (typically "user" or system user)
/// * `timestamp` - Creation timestamp
/// * `length` - Number of characters to use (3-8)
/// * `nonce` - Collision avoidance nonce
///
/// # Returns
/// A hash-based ID like "tazk-3s9". The suffix may be all digits; callers that
/// need a valid hash ID must use `generate_hash_id_with_collision_check`.
#[allow(clippy::too_many_arguments)]
pub fn generate_hash_id(
    title: &str,
    description: &str,
    creator: &str,
    timestamp: DateTime<Utc>,
    length: usize,
    nonce: u32,
) -> String {
    // Combine inputs into stable content string
    // Format: "title|description|creator|timestamp_nanos|nonce"
    let content = format!(
        "{}|{}|{}|{}|{}",
        title,
        description,
        creator,
        timestamp.timestamp_nanos_opt().unwrap_or(0),
        nonce
    );

    // Hash with SHA-256
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash_result = hasher.finalize();

    // Base36 encoding with variable length (3-8 chars)
    // Determine how many bytes to use based on desired output length
    let num_bytes = match length {
        3 => 2, // 2 bytes = 16 bits ≈ 3.09 base36 chars
        4 => 3, // 3 bytes = 24 bits ≈ 4.63 base36 chars
        5 => 4, // 4 bytes = 32 bits ≈ 6.18 base36 chars
        6 => 4, // 4 bytes = 32 bits ≈ 6.18 base36 chars
        7 => 5, // 5 bytes = 40 bits ≈ 7.73 base36 chars
        8 => 5, // 5 bytes = 40 bits ≈ 7.73 base36 chars
        _ => 3, // default to 3 bytes
    };
    let short_hash = encode_base36(&hash_result[..num_bytes], length);

    format!("{ID_PREFIX}{short_hash}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_generate_hash_id_basic() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();
        let id = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 0);

        // Should be format: prefix-hash
        assert!(id.starts_with(ID_PREFIX));
        assert_eq!(id.len(), ID_PREFIX.len() + 4); // prefix + dash + 4 base36 chars

        // Verify it's base36 (only contains 0-9, a-z)
        let hash_part = &id[ID_PREFIX.len()..];
        assert!(hash_part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_generate_hash_id_deterministic() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();

        let id1 = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 0);

        let id2 = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 0);

        // Same inputs should produce same hash
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_generate_hash_id_different_nonce() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();

        let id1 = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 0);

        let id2 = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 1);

        // Different nonce should produce different hash
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_generate_hash_id_different_lengths() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();

        // Base36 encoding supports lengths from 3-8
        for length in 3..=8 {
            let id = generate_hash_id(
                "First issue",
                "Test description",
                "user",
                timestamp,
                length,
                0,
            );

            assert!(id.starts_with(ID_PREFIX));
            assert_eq!(id.len(), ID_PREFIX.len() + length);

            // Verify it's base36 (only contains 0-9, a-z)
            let hash_part = &id[ID_PREFIX.len()..];
            assert!(hash_part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        }
    }

    #[test]
    fn test_generate_hash_id_different_inputs() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();

        let id1 = generate_hash_id("First issue", "Test description", "user", timestamp, 4, 0);

        let id2 = generate_hash_id(
            "Second issue", // Different title
            "Test description",
            "user",
            timestamp,
            4,
            0,
        );

        // Different inputs should produce different hash
        assert_ne!(id1, id2);
    }

    /// Find a (title, length) whose nonce-0 candidate is all digits.
    fn find_all_digit_title(timestamp: DateTime<Utc>, length: usize) -> String {
        (0..100_000)
            .map(|i| format!("title {i}"))
            .find(|title| {
                is_all_digit_suffix(&generate_hash_id(title, "d", "user", timestamp, length, 0))
            })
            .expect("an all-digit candidate exists within the search range")
    }

    #[test]
    fn all_digit_candidate_is_skipped() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();
        let title = find_all_digit_title(timestamp, 3);
        let bad = generate_hash_id(&title, "d", "user", timestamp, 3, 0);

        let mut seen = Vec::new();
        let id = generate_hash_id_with_collision_check(&title, "d", timestamp, 0, |c| {
            seen.push(c.to_string());
            false
        })
        .unwrap();

        assert_ne!(id, bad);
        assert!(!is_all_digit_suffix(&id));
        // The all-digit candidate is never offered to the collision checker.
        assert!(!seen.contains(&bad));
    }

    #[test]
    fn generated_ids_never_have_all_digit_suffix() {
        let timestamp = Utc.with_ymd_and_hms(2025, 10, 31, 12, 0, 0).unwrap();
        // Sizes chosen to hit initial lengths 3 through 8.
        for size in [0usize, 10, 100, 1000, 10_000, 100_000] {
            for i in 0..1700 {
                let id = generate_hash_id_with_collision_check(
                    &format!("issue {i}"),
                    "desc",
                    timestamp,
                    size,
                    |_| false,
                )
                .unwrap();
                let suffix = &id[ID_PREFIX.len()..];
                assert!(
                    suffix.bytes().any(|b| b.is_ascii_lowercase()),
                    "all-digit suffix: {id}"
                );
                assert!(suffix
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
            }
        }
    }
}
