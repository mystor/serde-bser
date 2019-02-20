use crate::error::{Error, Result};
use crate::Tag;

use byteorder::{ByteOrder, NativeEndian, ReadBytesExt};
use serde::de::{self, Expected, Unexpected};
use serde::forward_to_deserialize_any;
use std::borrow::Cow;
use std::cmp;
use std::io;
use std::marker::PhantomData;
use std::ops;
use std::str;

/// A structure that deserializes BSER into Rust values.
pub struct Deserializer<R, B = NativeEndian> {
    read: R,
    tag: Option<Tag>,
    scratch: Vec<u8>,
    _marker: PhantomData<B>,
}

impl<'de, R> Deserializer<IoRead<R>, NativeEndian>
where
    R: io::Read,
{
    /// Construct a deserializer for the given `io::Read`.
    #[inline]
    pub fn from_reader(read: R) -> Self {
        Self::new(IoRead::new(read))
    }
}

impl<'de> Deserializer<SliceRead<'de>, NativeEndian> {
    /// Construct a deserializer for the given byte slice.
    #[inline]
    pub fn from_slice(bytes: &'de [u8]) -> Self {
        Self::new(SliceRead::new(bytes))
    }
}

impl<'de, R> Deserializer<R, NativeEndian>
where
    R: Read<'de>,
{
    #[inline]
    pub fn native(read: R) -> Self {
        Self::new(read)
    }
}

impl<'de, R, B> Deserializer<R, B>
where
    R: Read<'de>,
    B: ByteOrder,
{
    /// Create a new deserializer with the given `Read` implementation.
    #[inline]
    pub fn new(read: R) -> Self {
        Deserializer {
            read,
            tag: None,
            scratch: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// The `Deserializer::end` method should be called after a value has been
    /// fully deserialized. This allows the `Deserializer` to validate that the
    /// input stream is at the end or that it only has trailing whitespace.
    #[inline]
    pub fn end(&mut self) -> Result<()> {
        match (self.tag, self.read.next()?) {
            (None, None) => Ok(()),
            _ => Err(Error::TrailingBytes),
        }
    }

    #[inline]
    fn peek_tag(&mut self) -> Result<Tag> {
        if let Some(tag) = self.tag {
            return Ok(tag);
        }

        let tag = match self.read.read_u8()? {
            0x00 => Tag::Array,
            0x01 => Tag::Object,
            0x02 => Tag::String,
            0x03 => Tag::Int8,
            0x04 => Tag::Int16,
            0x05 => Tag::Int32,
            0x06 => Tag::Int64,
            0x07 => Tag::Real,
            0x08 => Tag::True,
            0x09 => Tag::False,
            0x0a => Tag::Null,
            0x0b => Tag::Templated,
            0x0c => Tag::Missing,
            _ => return Err(Error::MalformedTag),
        };
        self.tag = Some(tag);
        Ok(tag)
    }

    #[inline]
    fn read_tag(&mut self) -> Result<Tag> {
        let tag = self.peek_tag()?;
        self.tag = None;
        Ok(tag)
    }

    #[inline]
    fn expect_tag(&mut self, tag: Tag, exp: &Expected) -> Result<()> {
        let actual = self.read_tag()?;
        if actual == tag {
            Ok(())
        } else {
            self.bad_tag(actual, exp)
        }
    }

    #[cold]
    fn bad_tag<T>(&mut self, tag: Tag, exp: &Expected) -> Result<T> {
        let unexp = match tag {
            Tag::Array => Unexpected::Seq,
            Tag::Object => Unexpected::Map,
            Tag::String => Unexpected::Bytes(match self.read_bytes()? {
                Reference::Borrowed(s) => s,
                Reference::Copied(s) => s,
            }),
            Tag::Int8 => Unexpected::Signed(self.read.read_i8()? as i64),
            Tag::Int16 => Unexpected::Signed(self.read.read_i16::<NativeEndian>()? as i64),
            Tag::Int32 => Unexpected::Signed(self.read.read_i32::<NativeEndian>()? as i64),
            Tag::Int64 => Unexpected::Signed(self.read.read_i64::<NativeEndian>()?),
            Tag::Real => Unexpected::Float(self.read.read_f64::<NativeEndian>()?),
            Tag::True => Unexpected::Bool(true),
            Tag::False => Unexpected::Bool(false),
            Tag::Null => Unexpected::Unit,
            Tag::Templated => Unexpected::Seq,
            Tag::Missing => Unexpected::Other("missing field"),
        };

        Err(de::Error::invalid_type(unexp, exp))
    }

    #[inline]
    fn read_usize(&mut self) -> Result<usize> {
        de::Deserialize::deserialize(self)
    }

    #[inline]
    fn read_bytes<'a>(&'a mut self) -> Result<Reference<'de, 'a, [u8]>> {
        let len = self.read_usize()?;
        self.read.read_ref(len, &mut self.scratch)
    }

    #[inline]
    fn scan_bytes<V>(&mut self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_bytes()? {
            Reference::Borrowed(s) => visitor.visit_borrowed_bytes(s),
            Reference::Copied(s) => visitor.visit_bytes(s),
        }
    }

    #[inline]
    fn scan_array<V>(&mut self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let len = self.read_usize()?;
        visitor.visit_seq(SeqAccess {
            de: self,
            remaining: len,
        })
    }

    #[inline]
    fn scan_templated<V>(&mut self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        // Read the array containing our keys.
        self.expect_tag(Tag::Array, &"template key array")?;

        let num_keys = self.read_usize()?;
        let mut keys = Vec::<Cow<'de, [u8]>>::with_capacity(num_keys);
        for _ in 0..num_keys {
            self.expect_tag(Tag::String, &"template object key")?;

            let key = match self.read_bytes()? {
                // XXX: We might be able to steal the scratch buffer?
                Reference::Copied(s) => Cow::Owned(s.to_owned()),
                Reference::Borrowed(s) => Cow::Borrowed(s),
            };
            keys.push(key);
        }

        // After names comes number of items.
        let len = self.read_usize()?;
        visitor.visit_seq(TemplatedAccess {
            de: self,
            keys: &keys,
            remaining: len,
        })
    }

    #[inline]
    fn scan_object<V>(&mut self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let len = self.read_usize()?;
        visitor.visit_map(MapAccess {
            de: self,
            remaining: len,
        })
    }

    #[inline]
    fn deserialize_prim_number<V>(&mut self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_tag()? {
            Tag::Int8 => visitor.visit_i8(self.read.read_i8()?),
            Tag::Int16 => visitor.visit_i16(self.read.read_i16::<NativeEndian>()?),
            Tag::Int32 => visitor.visit_i32(self.read.read_i32::<NativeEndian>()?),
            Tag::Int64 => visitor.visit_i64(self.read.read_i64::<NativeEndian>()?),
            Tag::Real => visitor.visit_f64(self.read.read_f64::<NativeEndian>()?),

            tag => self.bad_tag(tag, &"number"),
        }
    }
}

macro_rules! deserialize_prim_number {
    ($name:ident) => {
        #[inline]
        fn $name<V>(self, visitor: V) -> Result<V::Value>
        where
            V: de::Visitor<'de>,
        {
            self.deserialize_prim_number(visitor)
        }
    };
}

impl<'de, 'a, R, B> de::Deserializer<'de> for &'a mut Deserializer<R, B>
where
    R: Read<'de>,
    B: ByteOrder,
{
    type Error = Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_tag()? {
            Tag::Array => self.scan_array(visitor),
            Tag::Object => self.scan_object(visitor),
            Tag::String => self.scan_bytes(visitor),
            Tag::Int8 => visitor.visit_i8(self.read.read_i8()?),
            Tag::Int16 => visitor.visit_i16(self.read.read_i16::<NativeEndian>()?),
            Tag::Int32 => visitor.visit_i32(self.read.read_i32::<NativeEndian>()?),
            Tag::Int64 => visitor.visit_i64(self.read.read_i64::<NativeEndian>()?),
            Tag::Real => visitor.visit_f64(self.read.read_f64::<NativeEndian>()?),
            Tag::True => visitor.visit_bool(true),
            Tag::False => visitor.visit_bool(false),
            Tag::Null => visitor.visit_unit(),
            Tag::Templated => self.scan_templated(visitor),
            Tag::Missing => self.bad_tag(Tag::Missing, &"any value"),
        }
    }

    #[inline]
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_tag()? {
            Tag::True => visitor.visit_bool(true),
            Tag::False => visitor.visit_bool(false),

            tag => self.bad_tag(tag, &"boolean"),
        }
    }

    deserialize_prim_number!(deserialize_i8);
    deserialize_prim_number!(deserialize_i16);
    deserialize_prim_number!(deserialize_i32);
    deserialize_prim_number!(deserialize_i64);
    deserialize_prim_number!(deserialize_u8);
    deserialize_prim_number!(deserialize_u16);
    deserialize_prim_number!(deserialize_u32);
    deserialize_prim_number!(deserialize_u64);
    deserialize_prim_number!(deserialize_f32);
    deserialize_prim_number!(deserialize_f64);

    #[inline]
    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    #[inline]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_byte_buf(visitor)
    }

    #[inline]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.expect_tag(Tag::String, &"string")?;
        self.scan_bytes(visitor)
    }

    #[inline]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        if self.peek_tag()? == Tag::Null {
            self.tag = None;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    #[inline]
    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.expect_tag(Tag::Null, &"null")?;
        visitor.visit_unit()
    }

    #[inline]
    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_tag()? {
            Tag::Array => self.scan_array(visitor),
            Tag::Templated => self.scan_templated(visitor),

            tag => self.bad_tag(tag, &"array"),
        }
    }

    #[inline]
    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.expect_tag(Tag::Object, &"object")?;
        self.scan_object(visitor)
    }

    #[inline]
    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.read_tag()? {
            Tag::Array => self.scan_array(visitor),
            Tag::Templated => self.scan_templated(visitor),
            Tag::Object => self.scan_object(visitor),

            tag => self.bad_tag(tag, &"struct"),
        }
    }

    #[inline]
    fn deserialize_enum<V>(
        self,
        _name: &str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.peek_tag()? {
            // `{ "$key": $value }`-style variant
            Tag::Object => visitor.visit_enum(VariantAccess { de: self }),

            // "$key" style variant. Dispatch to StringLitAccess.
            Tag::String => {
                self.tag = None;
                let string = self.read_bytes()?;
                visitor.visit_enum(StringLitAccess { string })
            }

            tag => self.bad_tag(tag, &"enum variant"),
        }
    }

    #[inline]
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }
}

// ----------------------------------------------------------------------------

struct SeqAccess<'a, R: 'a, B>
where
    B: ByteOrder,
{
    de: &'a mut Deserializer<R, B>,
    remaining: usize,
}

impl<'de, 'a, R, B> de::SeqAccess<'de> for SeqAccess<'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        Ok(Some(seed.deserialize(&mut *self.de)?))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

// ----------------------------------------------------------------------------

struct MapAccess<'a, R: 'a, B> {
    de: &'a mut Deserializer<R, B>,
    remaining: usize,
}

impl<'de, 'a, R, B> de::MapAccess<'de> for MapAccess<'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;

        // Dispatch to a `StringLitAccess` to deserialize our object key.
        self.de.expect_tag(Tag::String, &"object key")?;
        let string = self.de.read_bytes()?;
        Ok(Some(seed.deserialize(StringLitAccess { string })?))
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

// ----------------------------------------------------------------------------

struct VariantAccess<'a, R: 'a, B> {
    de: &'a mut Deserializer<R, B>,
}

impl<'de, 'a, R, B> de::EnumAccess<'de> for VariantAccess<'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self)>
    where
        V: de::DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(&mut *self.de)?;
        Ok((variant, self))
    }
}

impl<'de, 'a, R, B> de::VariantAccess<'de> for VariantAccess<'a, R, B>
where
    R: Read<'de> + 'a,

    B: ByteOrder,
{
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        de::Deserialize::deserialize(self.de)
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(self.de)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self.de, len, visitor)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_struct(self.de, "", fields, visitor)
    }
}

// ----------------------------------------------------------------------------

/// Helper type used by StringLitAccess as the VariantAccess type when
/// deserializing a unit variant. Deserializes no data, but reports an
/// invalid_type error when attempting to deserialize non-unit variants.
struct DummyUnitVariantAccess;

impl<'de> de::VariantAccess<'de> for DummyUnitVariantAccess {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        Err(de::Error::invalid_type(
            Unexpected::UnitVariant,
            &"newtype variant",
        ))
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        Err(de::Error::invalid_type(
            Unexpected::UnitVariant,
            &"tuple variant",
        ))
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], _visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        Err(de::Error::invalid_type(
            Unexpected::UnitVariant,
            &"struct variant",
        ))
    }
}

/// Helper type for complex deserialization steps with single string literals.
/// This type can deserialize to unit variants, strings, bytes, etc.
struct StringLitAccess<'de, 'a> {
    string: Reference<'de, 'a, [u8]>,
}

impl<'de, 'a> de::EnumAccess<'de> for StringLitAccess<'de, 'a> {
    type Error = Error;
    type Variant = DummyUnitVariantAccess;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, DummyUnitVariantAccess)>
    where
        V: de::DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(self)?;
        Ok((variant, DummyUnitVariantAccess))
    }
}

impl<'de, 'a> de::Deserializer<'de> for StringLitAccess<'de, 'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.string {
            Reference::Borrowed(s) => visitor.visit_borrowed_bytes(s),
            Reference::Copied(s) => visitor.visit_bytes(s),
        }
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        // Map keys cannot be null.
        visitor.visit_some(self)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_enum<V>(
        self,
        _name: &str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_enum(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes
        byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

// ----------------------------------------------------------------------------

// SeqAccess for items within a templated sequence. Also implements
// Deserializer, used to deserialize each item in the sequence.
struct TemplatedAccess<'de, 'a, R, B> {
    de: &'a mut Deserializer<R, B>,
    keys: &'a [Cow<'de, [u8]>],
    remaining: usize,
}

impl<'de, 'a, R, B> de::SeqAccess<'de> for TemplatedAccess<'de, 'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        Ok(Some(seed.deserialize(self)?))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de, 'a, 'b, R, B> de::Deserializer<'de> for &'b mut TemplatedAccess<'de, 'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_map(TemplatedMapAccess {
            de: self.de,
            keys: self.keys.iter(),
        })
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        // All items are present in templated arrays
        visitor.visit_some(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes
        byte_buf unit unit_struct seq tuple enum tuple_struct map struct
        identifier ignored_any
    }
}

// `MapAccess` implementation for maps within a templated sequence.
struct TemplatedMapAccess<'de, 'a, R: 'a, B> {
    de: &'a mut Deserializer<R, B>,
    keys: std::slice::Iter<'a, Cow<'de, [u8]>>,
}

impl<'de, 'a, R, B> de::MapAccess<'de> for TemplatedMapAccess<'de, 'a, R, B>
where
    R: Read<'de> + 'a,
    B: ByteOrder,
{
    type Error = Error;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        // Loop over our keys until we find a non-missing value.
        while let Some(key) = self.keys.next() {
            // If we read in a Tag:Missing, skip it and move to the next key.
            if self.de.peek_tag()? == Tag::Missing {
                self.de.tag = None;
                continue;
            }

            // We've found a non-missing key, return it.
            return Ok(Some(seed.deserialize(StringLitAccess {
                string: match key {
                    Cow::Owned(s) => Reference::Copied(&s[..]),
                    Cow::Borrowed(s) => Reference::Borrowed(&s[..]),
                },
            })?));
        }

        Ok(None)
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }
}

// ----------------------------------------------------------------------------

pub enum Reference<'b, 'c, T: ?Sized + 'static> {
    Borrowed(&'b T),
    Copied(&'c T),
}

impl<'b, 'c, T: ?Sized + 'static> ops::Deref for Reference<'b, 'c, T> {
    type Target = T;

    fn deref(&self) -> &T {
        match *self {
            Reference::Borrowed(v) => v,
            Reference::Copied(v) => v,
        }
    }
}

// ----------------------------------------------------------------------------

/// This trait is similar to the trait from serde_json. It acts as a mechanism
/// for specializing byte slice cases to allow for borrowing deserializations.
///
/// This trait is sealed, and cannot be implemented by types outside of this
/// crate.
pub trait Read<'de>: private::Sealed + io::Read {
    #[doc(hidden)]
    fn next(&mut self) -> Result<Option<u8>>;

    #[doc(hidden)]
    fn read_ref<'s>(
        &mut self,
        len: usize,
        scratch: &'s mut Vec<u8>,
    ) -> Result<Reference<'de, 's, [u8]>>;
}

/// BSER input source which reads from an std::io::Read stream.
pub struct IoRead<R: io::Read> {
    read: R,
}

impl<R: io::Read> IoRead<R> {
    /// Create a new `io::Read` adapter.
    pub fn new(read: R) -> Self {
        IoRead { read }
    }
}

impl<'de, R: io::Read> Read<'de> for IoRead<R> {
    fn next(&mut self) -> Result<Option<u8>> {
        // Read a byte from the reader, and return it.
        let mut buf = [b'\0'; 1];
        let n = io::Read::read(&mut self.read, &mut buf)?;
        if n == 0 {
            Ok(None)
        } else {
            Ok(Some(buf[0]))
        }
    }

    fn read_ref<'s>(
        &mut self,
        len: usize,
        scratch: &'s mut Vec<u8>,
    ) -> Result<Reference<'de, 's, [u8]>> {
        // Grow our backing buffer to the correct size.
        scratch.resize(len, b'\0');
        io::Read::read_exact(&mut self.read, &mut scratch[..])?;
        Ok(Reference::Copied(&scratch[..]))
    }
}

impl<R: io::Read> io::Read for IoRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}

/// BSER input source which reads from a slice of bytes.
pub struct SliceRead<'de> {
    slice: &'de [u8],
    index: usize,
}

impl<'de> SliceRead<'de> {
    /// Create a new `&[u8]` adapter.
    pub fn new(slice: &'de [u8]) -> Self {
        SliceRead { slice, index: 0 }
    }
}

impl<'de> Read<'de> for SliceRead<'de> {
    fn next(&mut self) -> Result<Option<u8>> {
        if self.index < self.slice.len() {
            let ch = self.slice[self.index];
            self.index += 1;
            Ok(Some(ch))
        } else {
            Ok(None)
        }
    }

    fn read_ref<'s>(
        &mut self,
        len: usize,
        _scratch: &'s mut Vec<u8>,
    ) -> Result<Reference<'de, 's, [u8]>> {
        if let Some(end) = self.index.checked_add(len) {
            if end <= self.slice.len() {
                let bytes = &self.slice[self.index..end];
                self.index = end;
                return Ok(Reference::Borrowed(bytes));
            }
        }
        Err(io::Error::from(io::ErrorKind::UnexpectedEof).into())
    }
}

impl<'de> io::Read for SliceRead<'de> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rest = &self.slice[self.index..];

        let amt = cmp::min(buf.len(), rest.len());
        buf[..amt].copy_from_slice(&rest[..amt]);

        self.index += amt;
        Ok(amt)
    }
}

/// Prevent users from implementing the `Read` trait.
mod private {
    pub trait Sealed {}
}

impl<R> private::Sealed for IoRead<R> where R: io::Read {}
impl<'a> private::Sealed for SliceRead<'a> {}

// ----------------------------------------------------------------------------

/// Deserialize a `bser` value from an `io::Read`
pub fn from_reader<R, T>(rdr: R) -> Result<T>
where
    R: io::Read,
    T: de::DeserializeOwned,
{
    let mut de = Deserializer::native(IoRead::new(rdr));
    let value = de::Deserialize::deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// Deserialize a `bser` value from a byte slice
pub fn from_slice<'de, T>(v: &'de [u8]) -> Result<T>
where
    T: de::Deserialize<'de>,
{
    let mut de = Deserializer::native(SliceRead::new(v));
    let value = de::Deserialize::deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}
