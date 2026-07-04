//! Rust mirrors of the zvec C enumerations.
//!
//! Wherever possible, the raw typedefs from [`crate::sys`] are wrapped in
//! `#[repr(u32)]` Rust enums with a non-exhaustive `Other(u32)` variant to
//! guard against newer zvec releases introducing codes we don't recognise.

use crate::sys;

macro_rules! u32_enum {
    (
        $(#[$outer:meta])*
        pub enum $name:ident => $raw:ty {
            $( $(#[$var_attr:meta])* $variant:ident = $const:ident ),+ $(,)?
        }
    ) => {
        $(#[$outer])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$var_attr])* $variant ),+,
            /// A value not recognised by this bindings version.
            Other(u32),
        }

        impl $name {
            pub fn from_raw(value: $raw) -> Self {
                match value {
                    $( sys::$const => $name::$variant, )+
                    other => $name::Other(other),
                }
            }

            pub fn to_raw(self) -> $raw {
                match self {
                    $( $name::$variant => sys::$const, )+
                    $name::Other(n) => n,
                }
            }
        }
    };
}

u32_enum! {
    /// Scalar, vector, and array element types recognised by zvec.
    pub enum DataType => sys::zvec_data_type_t {
        Undefined = ZVEC_DATA_TYPE_UNDEFINED,
        Binary = ZVEC_DATA_TYPE_BINARY,
        String = ZVEC_DATA_TYPE_STRING,
        Bool = ZVEC_DATA_TYPE_BOOL,
        Int32 = ZVEC_DATA_TYPE_INT32,
        Int64 = ZVEC_DATA_TYPE_INT64,
        UInt32 = ZVEC_DATA_TYPE_UINT32,
        UInt64 = ZVEC_DATA_TYPE_UINT64,
        Float = ZVEC_DATA_TYPE_FLOAT,
        Double = ZVEC_DATA_TYPE_DOUBLE,
        VectorBinary32 = ZVEC_DATA_TYPE_VECTOR_BINARY32,
        VectorBinary64 = ZVEC_DATA_TYPE_VECTOR_BINARY64,
        VectorFp16 = ZVEC_DATA_TYPE_VECTOR_FP16,
        VectorFp32 = ZVEC_DATA_TYPE_VECTOR_FP32,
        VectorFp64 = ZVEC_DATA_TYPE_VECTOR_FP64,
        VectorInt4 = ZVEC_DATA_TYPE_VECTOR_INT4,
        VectorInt8 = ZVEC_DATA_TYPE_VECTOR_INT8,
        VectorInt16 = ZVEC_DATA_TYPE_VECTOR_INT16,
        SparseVectorFp16 = ZVEC_DATA_TYPE_SPARSE_VECTOR_FP16,
        SparseVectorFp32 = ZVEC_DATA_TYPE_SPARSE_VECTOR_FP32,
        ArrayBinary = ZVEC_DATA_TYPE_ARRAY_BINARY,
        ArrayString = ZVEC_DATA_TYPE_ARRAY_STRING,
        ArrayBool = ZVEC_DATA_TYPE_ARRAY_BOOL,
        ArrayInt32 = ZVEC_DATA_TYPE_ARRAY_INT32,
        ArrayInt64 = ZVEC_DATA_TYPE_ARRAY_INT64,
        ArrayUInt32 = ZVEC_DATA_TYPE_ARRAY_UINT32,
        ArrayUInt64 = ZVEC_DATA_TYPE_ARRAY_UINT64,
        ArrayFloat = ZVEC_DATA_TYPE_ARRAY_FLOAT,
        ArrayDouble = ZVEC_DATA_TYPE_ARRAY_DOUBLE,
    }
}

u32_enum! {
    /// Index algorithm families.
    pub enum IndexType => sys::zvec_index_type_t {
        Undefined = ZVEC_INDEX_TYPE_UNDEFINED,
        Hnsw = ZVEC_INDEX_TYPE_HNSW,
        Ivf = ZVEC_INDEX_TYPE_IVF,
        Flat = ZVEC_INDEX_TYPE_FLAT,
        Invert = ZVEC_INDEX_TYPE_INVERT,
    }
}

u32_enum! {
    /// Distance metric used by vector indexes.
    pub enum MetricType => sys::zvec_metric_type_t {
        Undefined = ZVEC_METRIC_TYPE_UNDEFINED,
        L2 = ZVEC_METRIC_TYPE_L2,
        Ip = ZVEC_METRIC_TYPE_IP,
        Cosine = ZVEC_METRIC_TYPE_COSINE,
        MipsL2 = ZVEC_METRIC_TYPE_MIPSL2,
    }
}

u32_enum! {
    /// Optional post-quantization applied to vectors before indexing.
    pub enum QuantizeType => sys::zvec_quantize_type_t {
        Undefined = ZVEC_QUANTIZE_TYPE_UNDEFINED,
        Fp16 = ZVEC_QUANTIZE_TYPE_FP16,
        Int8 = ZVEC_QUANTIZE_TYPE_INT8,
        Int4 = ZVEC_QUANTIZE_TYPE_INT4,
    }
}

/// Log level used by [`crate::config::LogConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
    Fatal = 4,
}

impl LogLevel {
    pub fn from_raw(value: sys::zvec_log_level_t) -> Self {
        match value.0 {
            0 => LogLevel::Debug,
            1 => LogLevel::Info,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            4 => LogLevel::Fatal,
            _ => LogLevel::Info,
        }
    }

    pub fn to_raw(self) -> sys::zvec_log_level_t {
        sys::zvec_log_level_t(self as u32)
    }
}

/// Log sink type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum LogType {
    Console = 0,
    File = 1,
}

impl LogType {
    pub fn from_raw(value: sys::zvec_log_type_t) -> Self {
        match value.0 {
            1 => LogType::File,
            _ => LogType::Console,
        }
    }
}

/// Document operator semantics carried on a [`crate::doc::Doc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum DocOperator {
    Insert = 0,
    Update = 1,
    Upsert = 2,
    Delete = 3,
}

impl DocOperator {
    pub fn from_raw(value: sys::zvec_doc_operator_t) -> Self {
        match value.0 {
            1 => DocOperator::Update,
            2 => DocOperator::Upsert,
            3 => DocOperator::Delete,
            _ => DocOperator::Insert,
        }
    }

    pub fn to_raw(self) -> sys::zvec_doc_operator_t {
        sys::zvec_doc_operator_t(self as u32)
    }
}

/// Human-readable description provided by zvec.
impl DataType {
    pub fn description(self) -> &'static str {
        unsafe {
            let ptr = sys::zvec_data_type_to_string(self.to_raw());
            crate::ffi_util::cstr_as_str(ptr).unwrap_or("")
        }
    }
}

impl IndexType {
    pub fn description(self) -> &'static str {
        unsafe {
            let ptr = sys::zvec_index_type_to_string(self.to_raw());
            crate::ffi_util::cstr_as_str(ptr).unwrap_or("")
        }
    }
}

impl MetricType {
    pub fn description(self) -> &'static str {
        unsafe {
            let ptr = sys::zvec_metric_type_to_string(self.to_raw());
            crate::ffi_util::cstr_as_str(ptr).unwrap_or("")
        }
    }
}
