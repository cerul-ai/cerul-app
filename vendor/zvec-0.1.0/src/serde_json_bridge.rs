//! JSON <-> [`Doc`] bridge, gated on the `serde-json` cargo feature.
//!
//! zvec field types are static (STRING, INT64, VECTOR_FP32, …) but JSON
//! is not — `[1, 2, 3]` alone could be a vector-fp32, an `array<int64>`,
//! or an `array<double>`. The bridge resolves this by requiring a
//! [`CollectionSchema`] that describes each field's type; unknown JSON
//! keys are rejected, and `"_pk"` is recognised as the primary key.
//!
//! ```no_run
//! # #[cfg(feature = "serde-json")]
//! # fn main() -> zvec::Result<()> {
//! use zvec::{CollectionSchema, DataType, Doc, FieldSchema};
//!
//! let schema = CollectionSchema::new("docs")?;
//! # let _ = schema;
//! # let _ = FieldSchema::new("id", DataType::String, false, 0)?;
//!
//! let value = serde_json::json!({
//!     "_pk": "doc1",
//!     "id": "doc1",
//!     "embedding": [0.1, 0.2, 0.3],
//! });
//! # let schema = CollectionSchema::new("docs")?;
//! let _doc = Doc::from_json(&value, &schema)?;
//! # Ok(()) }
//! # #[cfg(not(feature = "serde-json"))]
//! # fn main() {}
//! ```

use serde_json::Value;

use crate::doc::Doc;
use crate::error::{ErrorCode, Result, ZvecError};
use crate::schema::CollectionSchema;
use crate::types::DataType;

impl Doc {
    /// Build a [`Doc`] from a JSON object, resolving field types through
    /// `schema`. The special key `"_pk"` is recognised as the document's
    /// primary key when it's a string.
    ///
    /// Available with the `serde-json` cargo feature.
    #[cfg_attr(docsrs, doc(cfg(feature = "serde-json")))]
    pub fn from_json(value: &Value, schema: &CollectionSchema) -> Result<Self> {
        let obj = value.as_object().ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::InvalidArgument,
                "Doc::from_json expected a JSON object",
            )
        })?;

        let mut doc = Doc::new()?;
        for (key, v) in obj {
            if key == "_pk" {
                let pk = v.as_str().ok_or_else(|| {
                    ZvecError::with_message(ErrorCode::InvalidArgument, "`_pk` must be a string")
                })?;
                doc.set_pk(pk)?;
                continue;
            }
            let field = schema.field(key)?.ok_or_else(|| unknown_field(key))?;
            if v.is_null() {
                doc.set_field_null(key)?;
                continue;
            }
            write_field(&mut doc, key, field.data_type(), v)?;
        }
        Ok(doc)
    }
}

fn unknown_field(name: &str) -> ZvecError {
    ZvecError::with_message(
        ErrorCode::InvalidArgument,
        format!("unknown field `{name}` (not present in schema)"),
    )
}

fn invalid(msg: impl Into<String>) -> ZvecError {
    ZvecError::with_message(ErrorCode::InvalidArgument, msg.into())
}

fn write_field(doc: &mut Doc, name: &str, ty: DataType, v: &Value) -> Result<()> {
    match ty {
        DataType::String => {
            let s = v
                .as_str()
                .ok_or_else(|| invalid(format!("`{name}`: expected string")))?;
            doc.add_string(name, s)
        }
        DataType::Bool => {
            let b = v
                .as_bool()
                .ok_or_else(|| invalid(format!("`{name}`: expected bool")))?;
            doc.add_bool(name, b)
        }
        DataType::Int32 => doc.add_int32(name, as_i64(v, name)? as i32),
        DataType::Int64 => doc.add_int64(name, as_i64(v, name)?),
        DataType::UInt32 => doc.add_uint32(name, as_u64(v, name)? as u32),
        DataType::UInt64 => doc.add_uint64(name, as_u64(v, name)?),
        DataType::Float => doc.add_float(name, as_f64(v, name)? as f32),
        DataType::Double => doc.add_double(name, as_f64(v, name)?),
        DataType::VectorFp32 => {
            let xs = as_f32_array(v, name)?;
            doc.add_vector_fp32(name, &xs)
        }
        DataType::VectorFp64 => {
            let xs = as_f64_array(v, name)?;
            doc.add_vector_fp64(name, &xs)
        }
        DataType::VectorInt8 => {
            let xs: Vec<i8> = as_i64_array(v, name)?
                .into_iter()
                .map(|x| x as i8)
                .collect();
            doc.add_vector_int8(name, &xs)
        }
        DataType::VectorInt16 => {
            let xs: Vec<i16> = as_i64_array(v, name)?
                .into_iter()
                .map(|x| x as i16)
                .collect();
            doc.add_vector_int16(name, &xs)
        }
        DataType::ArrayInt32 => {
            let xs: Vec<i32> = as_i64_array(v, name)?
                .into_iter()
                .map(|x| x as i32)
                .collect();
            doc.add_array_int32(name, &xs)
        }
        DataType::ArrayInt64 => {
            let xs = as_i64_array(v, name)?;
            doc.add_array_int64(name, &xs)
        }
        DataType::ArrayUInt32 => {
            let xs: Vec<u32> = as_u64_array(v, name)?
                .into_iter()
                .map(|x| x as u32)
                .collect();
            doc.add_array_uint32(name, &xs)
        }
        DataType::ArrayUInt64 => {
            let xs = as_u64_array(v, name)?;
            doc.add_array_uint64(name, &xs)
        }
        DataType::ArrayFloat => {
            let xs = as_f32_array(v, name)?;
            doc.add_array_float(name, &xs)
        }
        DataType::ArrayDouble => {
            let xs = as_f64_array(v, name)?;
            doc.add_array_double(name, &xs)
        }
        DataType::Binary => {
            // Accept either a JSON string (treated as bytes) or an array of
            // small ints (each byte).
            if let Some(s) = v.as_str() {
                doc.add_binary(name, s.as_bytes())
            } else if let Some(arr) = v.as_array() {
                let mut bytes = Vec::with_capacity(arr.len());
                for x in arr {
                    let b = x.as_u64().ok_or_else(|| {
                        invalid(format!("`{name}`: expected array of byte-sized ints"))
                    })?;
                    if b > u8::MAX as u64 {
                        return Err(invalid(format!("`{name}`: byte value {b} out of u8 range")));
                    }
                    bytes.push(b as u8);
                }
                doc.add_binary(name, &bytes)
            } else {
                Err(invalid(format!(
                    "`{name}`: expected string or array for BINARY"
                )))
            }
        }
        other => Err(invalid(format!(
            "`{name}`: data type {other:?} is not supported by Doc::from_json \
             (use add_field_raw / add_vector_* on the typed helpers)"
        ))),
    }
}

fn as_i64(v: &Value, name: &str) -> Result<i64> {
    v.as_i64()
        .ok_or_else(|| invalid(format!("`{name}`: expected integer")))
}

fn as_u64(v: &Value, name: &str) -> Result<u64> {
    v.as_u64()
        .ok_or_else(|| invalid(format!("`{name}`: expected unsigned integer")))
}

fn as_f64(v: &Value, name: &str) -> Result<f64> {
    v.as_f64()
        .ok_or_else(|| invalid(format!("`{name}`: expected number")))
}

fn as_f32_array(v: &Value, name: &str) -> Result<Vec<f32>> {
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(format!("`{name}`: expected array")))?;
    arr.iter()
        .map(|x| {
            x.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| invalid(format!("`{name}`: expected number in array")))
        })
        .collect()
}

fn as_f64_array(v: &Value, name: &str) -> Result<Vec<f64>> {
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(format!("`{name}`: expected array")))?;
    arr.iter()
        .map(|x| {
            x.as_f64()
                .ok_or_else(|| invalid(format!("`{name}`: expected number in array")))
        })
        .collect()
}

fn as_i64_array(v: &Value, name: &str) -> Result<Vec<i64>> {
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(format!("`{name}`: expected array")))?;
    arr.iter()
        .map(|x| {
            x.as_i64()
                .ok_or_else(|| invalid(format!("`{name}`: expected integer in array")))
        })
        .collect()
}

fn as_u64_array(v: &Value, name: &str) -> Result<Vec<u64>> {
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(format!("`{name}`: expected array")))?;
    arr.iter()
        .map(|x| {
            x.as_u64()
                .ok_or_else(|| invalid(format!("`{name}`: expected unsigned integer in array")))
        })
        .collect()
}
