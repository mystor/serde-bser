//! Serde support for the BSER Binary Protocol supported by Watchman

pub mod error;
pub mod ser;
pub mod de;

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Tag {
    Array = 0x00,
    Object = 0x01,
    String = 0x02,
    Int8 = 0x03,
    Int16 = 0x04,
    Int32 = 0x05,
    Int64 = 0x06,
    Real = 0x07,
    True = 0x08,
    False = 0x09,
    Null = 0x0a,
    Templated = 0x0b,
    Missing = 0x0c,
}