use serde_bser::ser::to_vec;
use serde_bser::de::from_slice;
use serde::{Serialize, Deserialize};
use serde_derive::{Serialize, Deserialize};
use std::collections::BTreeMap;

use std::slice;
use std::mem;
use std::fmt;

type Test = Result<(), Box<std::error::Error>>;

const TAG_ARRAY: &[u8] = &[0x00];
const TAG_OBJECT: &[u8] = &[0x01];
const TAG_STRING: &[u8] = &[0x02];
const TAG_INT8: &[u8] = &[0x03];
const TAG_INT16: &[u8] = &[0x04];
const TAG_INT32: &[u8] = &[0x05];
const TAG_INT64: &[u8] = &[0x06];
const TAG_REAL: &[u8] = &[0x07];
const TAG_TRUE: &[u8] = &[0x08];
const TAG_FALSE: &[u8] = &[0x09];
const TAG_NULL: &[u8] = &[0x0a];
const TAG_TEMPLATED: &[u8] = &[0x0b];
const TAG_MISSING: &[u8] = &[0x0c];

fn test_known<'de, T>(rust: &T, bser: &'de [u8]) -> Test
where
    T: Serialize + Deserialize<'de>,
    T: PartialEq<T> + fmt::Debug,
{
    // Test Serialization
    eprintln!("Serialization");
    let serialized = to_vec(rust)?;
    assert_eq!(&serialized[..], bser, "serialization matches");
    eprintln!("=> OK");

    // Test Deserialization
    eprintln!("Deserialization");
    let deserialized: T = from_slice(bser)?;
    assert_eq!(&deserialized, rust, "deserialization matches");
    eprintln!("=> OK");

    Ok(())
}

macro_rules! serialize_test {
    ($name:ident : $json:expr => [$($out:expr),* $(,)*]) => {
        #[test]
        fn $name() -> Test {
            // Initial JSON to use as input.
            let input = $json;

            // Build output vector to compare to.
            let mut expected = Vec::<u8>::new();
            $(
                expected.extend(&$out[..]);
            )*

            test_known(&input, &expected)
        }
    };
}

fn bytes<T: Copy>(x: T) -> Vec<u8> {
    unsafe {
        slice::from_raw_parts(&x as *const T as *const u8, mem::size_of::<T>()).to_owned()
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct BasicObject {
    name: String,
    age: i32,
    year: i32,
}

serialize_test!(
    basic_object: BasicObject{
        name: "John Doe".to_owned(),
        age: 43,
        year: 1976
    } => [
        TAG_OBJECT, TAG_INT8, [3],
        TAG_STRING, TAG_INT8, [4], b"name",
        TAG_STRING, TAG_INT8, [8], b"John Doe",
        TAG_STRING, TAG_INT8, [3], b"age",
        TAG_INT8, [43],
        TAG_STRING, TAG_INT8, [4], b"year",
        TAG_INT16, bytes(1976_i16),
    ]
);

serialize_test!(
    map_test: {
        let mut map = BTreeMap::<String, i64>::new();
        map.insert("aaa".to_owned(), 10);
        map.insert("bbb".to_owned(), 20);
        map.insert("ccc".to_owned(), 0xdeadbeef);
        map.insert("ddd".to_owned(), -300);
        map
    } => [
        TAG_OBJECT, TAG_INT8, [4],
        TAG_STRING, TAG_INT8, [3], b"aaa",
        TAG_INT8, [10],
        TAG_STRING, TAG_INT8, [3], b"bbb",
        TAG_INT8, [20],
        TAG_STRING, TAG_INT8, [3], b"ccc",
        TAG_INT64, bytes(0xdeadbeef_i64),
        TAG_STRING, TAG_INT8, [3], b"ddd",
        TAG_INT16, bytes(-300_i16),
    ]
);
