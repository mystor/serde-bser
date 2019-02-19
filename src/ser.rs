use crate::error::{Error, Result};
use crate::Tag;

use byteorder::{ByteOrder, NativeEndian, WriteBytesExt};
use serde::ser;
use std::io;
use std::marker::PhantomData;

/// Helper object for serializing Rust objects into BSER.
pub struct Serializer<W, B = NativeEndian>
where
    B: ByteOrder,
{
    writer: W,
    _marker: PhantomData<B>,
}

impl<W> Serializer<W, NativeEndian>
where
    W: io::Write,
{
    #[inline]
    pub fn native(writer: W) -> Self {
        Self::new(writer)
    }
}

impl<W, B> Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    #[inline]
    pub fn new(writer: W) -> Self {
        Serializer {
            writer,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn write_tag(&mut self, tag: Tag) -> Result<()> {
        self.writer.write_u8(tag as u8)?;
        Ok(())
    }

    #[inline]
    fn serialize_usize(&mut self, v: usize) -> Result<()> {
        ser::Serializer::serialize_u64(self, v as u64)
    }

    #[inline]
    fn serialize_int(&mut self, v: i64) -> Result<()> {
        // Find the smallest integer value we can write out
        if (std::i8::MIN as i64) <= v && v <= (std::i8::MAX as i64) {
            self.write_tag(Tag::Int8)?;
            self.writer.write_i8(v as i8)?;
        } else if (std::i16::MIN as i64) <= v && v <= (std::i16::MAX as i64) {
            self.write_tag(Tag::Int16)?;
            self.writer.write_i16::<B>(v as i16)?;
        } else if (std::i32::MIN as i64) <= v && v <= (std::i32::MAX as i64) {
            self.write_tag(Tag::Int32)?;
            self.writer.write_i32::<B>(v as i32)?;
        } else {
            self.write_tag(Tag::Int64)?;
            self.writer.write_i64::<B>(v as i64)?;
        }
        Ok(())
    }

    #[inline]
    fn begin_object(&mut self, size: usize) -> Result<()> {
        self.write_tag(Tag::Object)?;
        self.serialize_usize(size)
    }

    #[inline]
    fn begin_array(&mut self, size: usize) -> Result<()> {
        self.write_tag(Tag::Array)?;
        self.serialize_usize(size)
    }
}

impl<'a, W, B> ser::Serializer for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    #[inline]
    fn serialize_bool(self, v: bool) -> Result<()> {
        if v {
            self.write_tag(Tag::True)
        } else {
            self.write_tag(Tag::False)
        }
    }

    #[inline]
    fn serialize_i8(self, v: i8) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_i16(self, v: i16) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_i32(self, v: i32) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_i64(self, v: i64) -> Result<()> {
        self.serialize_int(v)
    }

    #[inline]
    fn serialize_u8(self, v: u8) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_u16(self, v: u16) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_u64(self, v: u64) -> Result<()> {
        if v > i64::max_value() as u64 {
            return Err(Error::IntegerOverflow);
        }
        self.serialize_int(v as i64)
    }

    #[inline]
    fn serialize_f32(self, v: f32) -> Result<()> {
        self.serialize_f64(v as f64)
    }

    #[inline]
    fn serialize_f64(self, v: f64) -> Result<()> {
        self.write_tag(Tag::Real)?;
        self.writer.write_f64::<B>(v)?;
        Ok(())
    }

    #[inline]
    fn serialize_char(self, v: char) -> Result<()> {
        let mut buf = [0; 4];
        self.serialize_str(v.encode_utf8(&mut buf))
    }

    #[inline]
    fn serialize_str(self, v: &str) -> Result<()> {
        self.serialize_bytes(v.as_bytes())
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.write_tag(Tag::String)?;
        self.serialize_usize(v.len())?;
        self.writer.write(v)?;
        Ok(())
    }

    #[inline]
    fn serialize_unit(self) -> Result<()> {
        self.write_tag(Tag::Null)
    }

    #[inline]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.serialize_str(variant)
    }

    /// Serialize newtypes without an object wrapper.
    #[inline]
    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()>
    where
        T: ser::Serialize,
    {
        self.begin_object(1)?;
        self.serialize_str(variant)?;
        value.serialize(self)?;
        Ok(())
    }

    #[inline]
    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_some<T: ?Sized>(self, value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        if let Some(len) = len {
            self.begin_array(len)?;
            Ok(self)
        } else {
            Err(Error::LengthRequired)
        }
    }

    #[inline]
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.begin_object(1)?;
        self.serialize_str(variant)?;
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        if let Some(len) = len {
            self.begin_object(len)?;
            Ok(self)
        } else {
            Err(Error::LengthRequired)
        }
    }

    #[inline]
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.begin_object(1)?;
        self.serialize_str(variant)?;
        self.serialize_map(Some(len))
    }
}

impl<'a, W, B> ser::SerializeSeq for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<T: ?Sized>(&mut self, v: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        v.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeTuple for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<T: ?Sized>(&mut self, v: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        ser::SerializeSeq::serialize_element(self, v)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeTupleStruct for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T: ?Sized>(&mut self, v: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        ser::SerializeSeq::serialize_element(self, v)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeTupleVariant for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T: ?Sized>(&mut self, v: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        v.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeMap for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        // NOTE: Use a custom sub-serializer here to convert any keys to
        // strings, and reject other keys.
        key.serialize(MapKeySerializer { ser: &mut **self })
    }

    #[inline]
    fn serialize_value<T: ?Sized>(&mut self, v: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        v.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeStruct for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        // XXX(nika): This can probably do better!
        ser::Serializer::serialize_str(&mut **self, key)?;
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W, B> ser::SerializeStructVariant for &'a mut Serializer<W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        ser::SerializeStruct::serialize_field(self, key, value)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

/// Helper serializer for map keys to ensure that they are valid strings.
struct MapKeySerializer<'a, W: 'a, B>
where
    B: ByteOrder,
{
    ser: &'a mut Serializer<W, B>,
}

impl<'a, W, B> MapKeySerializer<'a, W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    fn serialize_int(self, value: impl itoa::Integer) -> Result<()> {
        let mut bytes = [b'\0'; 20];
        let n = itoa::write(&mut bytes[..], value)?;
        ser::Serializer::serialize_bytes(self.ser, &bytes[..n])
    }
}

impl<'a, W, B> ser::Serializer for MapKeySerializer<'a, W, B>
where
    W: io::Write,
    B: ByteOrder,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;
    type SerializeTupleStruct = ser::Impossible<(), Error>;
    type SerializeTupleVariant = ser::Impossible<(), Error>;
    type SerializeMap = ser::Impossible<(), Error>;
    type SerializeStruct = ser::Impossible<(), Error>;
    type SerializeStructVariant = ser::Impossible<(), Error>;

    #[inline]
    fn serialize_str(self, value: &str) -> Result<()> {
        self.ser.serialize_str(value)
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.ser.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        value.serialize(self)
    }

    fn serialize_bool(self, _value: bool) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_i8(self, value: i8) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_i16(self, value: i16) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_i32(self, value: i32) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_i64(self, value: i64) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_u8(self, value: u8) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_u16(self, value: u16) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_u32(self, value: u32) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_u64(self, value: u64) -> Result<()> {
        self.serialize_int(value)
    }

    fn serialize_f32(self, _value: f32) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_f64(self, _value: f64) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_char(self, value: char) -> Result<()> {
        self.ser.serialize_char(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<()> {
        // XXX: Support bytes in keys? Not technically disallowed, but
        // explicitly discouraged.
        self.ser.serialize_bytes(value)
    }

    fn serialize_unit(self) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<()>
    where
        T: ser::Serialize,
    {
        Err(Error::NonStringKey)
    }

    fn serialize_none(self) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_some<T: ?Sized>(self, _value: &T) -> Result<()>
    where
        T: ser::Serialize,
    {
        Err(Error::NonStringKey)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::NonStringKey)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Err(Error::NonStringKey)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(Error::NonStringKey)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::NonStringKey)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(Error::NonStringKey)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Err(Error::NonStringKey)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::NonStringKey)
    }
}

// ----------------------------------------------------------------------------

/// Serialize the given data structure as BSER into the IO stream.
///
/// # Errors
///
/// Serialization can fail if `T`'s implementation of `Serialize` decides to
/// fail, or if `T` contains a map with non-string keys.
pub fn to_writer<W, T: ?Sized>(writer: W, value: &T) -> Result<()>
where
    W: io::Write,
    T: ser::Serialize,
{
    let mut ser = Serializer::native(writer);
    value.serialize(&mut ser)?;
    Ok(())
}

/// Serialize the given data structure as a BSER byte vector.
///
/// # Errors
///
/// Serialization can fail if `T`'s implementation of `Serialize` decides to
/// fail, or if `T` contains a map with non-string keys.
pub fn to_vec<T: ?Sized>(value: &T) -> Result<Vec<u8>>
where
    T: ser::Serialize,
{
    let mut writer = Vec::with_capacity(128);
    to_writer(&mut writer, value)?;
    Ok(writer)
}
