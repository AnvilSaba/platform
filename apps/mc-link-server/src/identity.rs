use nutype::nutype;

#[nutype(
    validate(
        not_empty,
        len_char_max = 16,
        predicate = |name: &str| name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    ),
    derive(Debug, PartialEq, Clone, AsRef, Deserialize)
)]
pub(crate) struct Name(String);
