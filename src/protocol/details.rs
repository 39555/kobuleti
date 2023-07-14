



 macro_rules! unwrap_enum {
    ($enum:expr, $value:path) => (
        match $enum {
            $value(x) =>Some(x),
            _ => None,
        }
    )
}
pub(crate) use unwrap_enum;


