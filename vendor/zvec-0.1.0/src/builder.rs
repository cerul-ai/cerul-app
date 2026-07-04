//! Builder APIs over [`CollectionSchema`], [`FieldSchema`], and
//! [`VectorQuery`].
//!
//! The C-style `new` + `set_*` surface is still available on every type;
//! builders are a thin, pure-Rust layer that composes well and is
//! friendlier in examples and docs.
//!
//! ```no_run
//! # fn main() -> zvec::Result<()> {
//! use zvec::{CollectionSchema, FieldSchema, MetricType, VectorQuery};
//!
//! let schema = CollectionSchema::builder("docs")
//!     .field(FieldSchema::string("id").invert_index(true, false))
//!     .field(
//!         FieldSchema::vector_fp32("embedding", 3)
//!             .hnsw(16, 200)
//!             .metric(MetricType::Cosine),
//!     )
//!     .build()?;
//!
//! let query = VectorQuery::builder()
//!     .field("embedding")
//!     .vector_fp32(&[0.1, 0.2, 0.3])
//!     .topk(10)
//!     .include_vector(true)
//!     .build()?;
//! # let _ = (schema, query);
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::index_params::IndexParams;
use crate::query::VectorQuery;
use crate::query_params::{FlatQueryParams, HnswQueryParams, IvfQueryParams};
use crate::schema::{CollectionSchema, FieldSchema};
use crate::types::{DataType, IndexType, MetricType, QuantizeType};

// -----------------------------------------------------------------------------
// CollectionSchemaBuilder
// -----------------------------------------------------------------------------

/// Builder for [`CollectionSchema`]. Accumulates pending fields and
/// options; surfaces any validation errors at [`Self::build`].
pub struct CollectionSchemaBuilder {
    name: String,
    fields: Vec<FieldSchemaBuilder>,
    max_doc_count_per_segment: Option<u64>,
}

impl CollectionSchemaBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
            max_doc_count_per_segment: None,
        }
    }

    /// Add a field. Equivalent to calling [`FieldSchemaBuilder::build`]
    /// and [`CollectionSchema::add_field`].
    pub fn field(mut self, field: FieldSchemaBuilder) -> Self {
        self.fields.push(field);
        self
    }

    pub fn max_doc_count_per_segment(mut self, n: u64) -> Self {
        self.max_doc_count_per_segment = Some(n);
        self
    }

    /// Finalize the builder.
    pub fn build(self) -> Result<CollectionSchema> {
        let mut schema = CollectionSchema::new(&self.name)?;
        for fb in self.fields {
            let f = fb.build()?;
            schema.add_field(&f)?;
        }
        if let Some(n) = self.max_doc_count_per_segment {
            schema.set_max_doc_count_per_segment(n)?;
        }
        Ok(schema)
    }
}

// -----------------------------------------------------------------------------
// FieldSchemaBuilder
// -----------------------------------------------------------------------------

/// Builder for [`FieldSchema`]. Constructed via the type-specific
/// associated functions (`FieldSchema::string`, `::vector_fp32`, ...).
pub struct FieldSchemaBuilder {
    name: String,
    data_type: DataType,
    dimension: u32,
    nullable: bool,
    index: IndexSpec,
}

enum IndexSpec {
    None,
    Hnsw {
        m: i32,
        ef_construction: i32,
        metric: Option<MetricType>,
        quantize: Option<QuantizeType>,
    },
    Ivf {
        n_list: i32,
        n_iters: i32,
        use_soar: bool,
        metric: Option<MetricType>,
        quantize: Option<QuantizeType>,
    },
    Flat {
        metric: Option<MetricType>,
        quantize: Option<QuantizeType>,
    },
    Invert {
        enable_range_opt: bool,
        enable_wildcard: bool,
    },
    Explicit(IndexParams),
}

impl FieldSchemaBuilder {
    fn new(name: impl Into<String>, data_type: DataType, dimension: u32) -> Self {
        Self {
            name: name.into(),
            data_type,
            dimension,
            nullable: false,
            index: IndexSpec::None,
        }
    }

    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    // --- index-type shortcuts ---

    /// Attach an HNSW index. Chain `.metric(...)` or `.quantize(...)` to
    /// tune further.
    pub fn hnsw(mut self, m: i32, ef_construction: i32) -> Self {
        self.index = IndexSpec::Hnsw {
            m,
            ef_construction,
            metric: None,
            quantize: None,
        };
        self
    }

    pub fn ivf(mut self, n_list: i32, n_iters: i32, use_soar: bool) -> Self {
        self.index = IndexSpec::Ivf {
            n_list,
            n_iters,
            use_soar,
            metric: None,
            quantize: None,
        };
        self
    }

    pub fn flat(mut self) -> Self {
        self.index = IndexSpec::Flat {
            metric: None,
            quantize: None,
        };
        self
    }

    pub fn invert_index(mut self, enable_range_opt: bool, enable_wildcard: bool) -> Self {
        self.index = IndexSpec::Invert {
            enable_range_opt,
            enable_wildcard,
        };
        self
    }

    /// Install a pre-built [`IndexParams`] directly, bypassing the shortcut
    /// methods. Useful when the caller wants to reuse an `IndexParams`
    /// across several fields.
    pub fn index_params(mut self, p: IndexParams) -> Self {
        self.index = IndexSpec::Explicit(p);
        self
    }

    // --- vector-index tuning ---

    /// Set the distance metric on a vector index. No-op for invert /
    /// unindexed fields.
    pub fn metric(mut self, metric: MetricType) -> Self {
        match &mut self.index {
            IndexSpec::Hnsw { metric: m, .. }
            | IndexSpec::Ivf { metric: m, .. }
            | IndexSpec::Flat { metric: m, .. } => *m = Some(metric),
            _ => {}
        }
        self
    }

    pub fn quantize(mut self, q: QuantizeType) -> Self {
        match &mut self.index {
            IndexSpec::Hnsw { quantize, .. }
            | IndexSpec::Ivf { quantize, .. }
            | IndexSpec::Flat { quantize, .. } => *quantize = Some(q),
            _ => {}
        }
        self
    }

    /// Finalize. Called implicitly by [`CollectionSchemaBuilder::field`].
    pub fn build(self) -> Result<FieldSchema> {
        let mut f = FieldSchema::new(&self.name, self.data_type, self.nullable, self.dimension)?;
        match self.index {
            IndexSpec::None => {}
            IndexSpec::Explicit(p) => f.set_index_params(&p)?,
            IndexSpec::Hnsw {
                m,
                ef_construction,
                metric,
                quantize,
            } => {
                let mut p = IndexParams::new(IndexType::Hnsw)?;
                p.set_hnsw_params(m, ef_construction)?;
                if let Some(m) = metric {
                    p.set_metric_type(m)?;
                }
                if let Some(q) = quantize {
                    p.set_quantize_type(q)?;
                }
                f.set_index_params(&p)?;
            }
            IndexSpec::Ivf {
                n_list,
                n_iters,
                use_soar,
                metric,
                quantize,
            } => {
                let mut p = IndexParams::new(IndexType::Ivf)?;
                p.set_ivf_params(n_list, n_iters, use_soar)?;
                if let Some(m) = metric {
                    p.set_metric_type(m)?;
                }
                if let Some(q) = quantize {
                    p.set_quantize_type(q)?;
                }
                f.set_index_params(&p)?;
            }
            IndexSpec::Flat { metric, quantize } => {
                let mut p = IndexParams::new(IndexType::Flat)?;
                if let Some(m) = metric {
                    p.set_metric_type(m)?;
                }
                if let Some(q) = quantize {
                    p.set_quantize_type(q)?;
                }
                f.set_index_params(&p)?;
            }
            IndexSpec::Invert {
                enable_range_opt,
                enable_wildcard,
            } => {
                let mut p = IndexParams::new(IndexType::Invert)?;
                p.set_invert_params(enable_range_opt, enable_wildcard)?;
                f.set_index_params(&p)?;
            }
        }
        Ok(f)
    }
}

// -----------------------------------------------------------------------------
// Associated constructors on CollectionSchema / FieldSchema
// -----------------------------------------------------------------------------

impl CollectionSchema {
    /// Start building a collection schema with the given name.
    pub fn builder(name: impl Into<String>) -> CollectionSchemaBuilder {
        CollectionSchemaBuilder::new(name)
    }
}

/// Typed constructors for [`FieldSchemaBuilder`].
///
/// These cover the data types commonly used in documents; for rarer
/// types call [`FieldSchema::custom`] or build a [`FieldSchema`]
/// directly with [`FieldSchema::new`].
impl FieldSchema {
    pub fn string(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::String, 0)
    }
    pub fn binary(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Binary, 0)
    }
    pub fn bool(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Bool, 0)
    }
    pub fn int32(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Int32, 0)
    }
    pub fn int64(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Int64, 0)
    }
    pub fn uint32(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::UInt32, 0)
    }
    pub fn uint64(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::UInt64, 0)
    }
    pub fn float(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Float, 0)
    }
    pub fn double(name: impl Into<String>) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::Double, 0)
    }
    pub fn vector_fp32(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorFp32, dim)
    }
    pub fn vector_fp64(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorFp64, dim)
    }
    pub fn vector_fp16(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorFp16, dim)
    }
    pub fn vector_int8(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorInt8, dim)
    }
    pub fn vector_int16(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorInt16, dim)
    }
    pub fn vector_int4(name: impl Into<String>, dim: u32) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, DataType::VectorInt4, dim)
    }
    /// Escape hatch for any other [`DataType`].
    pub fn custom(
        name: impl Into<String>,
        data_type: DataType,
        dimension: u32,
    ) -> FieldSchemaBuilder {
        FieldSchemaBuilder::new(name, data_type, dimension)
    }
}

// -----------------------------------------------------------------------------
// VectorQueryBuilder
// -----------------------------------------------------------------------------

/// Builder for [`VectorQuery`]. Covers every knob on the query except
/// `set_query_params` (the untyped escape hatch); HNSW / IVF / Flat
/// parameters still go through their respective builders and
/// `.params(...)`.
pub struct VectorQueryBuilder {
    field: Option<String>,
    topk: Option<i32>,
    filter: Option<String>,
    include_vector: Option<bool>,
    include_doc_id: Option<bool>,
    output_fields: Option<Vec<String>>,
    vector_bytes: Option<Vec<u8>>,
    params: Option<QueryParamsKind>,
}

enum QueryParamsKind {
    Hnsw(HnswQueryParams),
    Ivf(IvfQueryParams),
    Flat(FlatQueryParams),
}

impl VectorQuery {
    /// Start building a vector query.
    pub fn builder() -> VectorQueryBuilder {
        VectorQueryBuilder::new()
    }
}

impl VectorQueryBuilder {
    fn new() -> Self {
        Self {
            field: None,
            topk: None,
            filter: None,
            include_vector: None,
            include_doc_id: None,
            output_fields: None,
            vector_bytes: None,
            params: None,
        }
    }

    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.field = Some(name.into());
        self
    }

    pub fn topk(mut self, k: i32) -> Self {
        self.topk = Some(k);
        self
    }

    pub fn filter(mut self, expr: impl Into<String>) -> Self {
        self.filter = Some(expr.into());
        self
    }

    pub fn include_vector(mut self, include: bool) -> Self {
        self.include_vector = Some(include);
        self
    }

    pub fn include_doc_id(mut self, include: bool) -> Self {
        self.include_doc_id = Some(include);
        self
    }

    pub fn output_fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.output_fields = Some(fields.into_iter().map(Into::into).collect());
        self
    }

    pub fn vector_fp32(mut self, vec: &[f32]) -> Self {
        let mut bytes = Vec::with_capacity(std::mem::size_of_val(vec));
        for v in vec {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        self.vector_bytes = Some(bytes);
        self
    }

    pub fn vector_fp64(mut self, vec: &[f64]) -> Self {
        let mut bytes = Vec::with_capacity(std::mem::size_of_val(vec));
        for v in vec {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        self.vector_bytes = Some(bytes);
        self
    }

    /// Raw vector bytes. Prefer [`Self::vector_fp32`] / [`Self::vector_fp64`]
    /// when possible.
    pub fn vector_raw(mut self, bytes: Vec<u8>) -> Self {
        self.vector_bytes = Some(bytes);
        self
    }

    pub fn hnsw_params(mut self, params: HnswQueryParams) -> Self {
        self.params = Some(QueryParamsKind::Hnsw(params));
        self
    }

    pub fn ivf_params(mut self, params: IvfQueryParams) -> Self {
        self.params = Some(QueryParamsKind::Ivf(params));
        self
    }

    pub fn flat_params(mut self, params: FlatQueryParams) -> Self {
        self.params = Some(QueryParamsKind::Flat(params));
        self
    }

    pub fn build(self) -> Result<VectorQuery> {
        let mut q = VectorQuery::new()?;
        if let Some(name) = self.field {
            q.set_field_name(&name)?;
        }
        if let Some(k) = self.topk {
            q.set_topk(k)?;
        }
        if let Some(f) = self.filter {
            q.set_filter(&f)?;
        }
        if let Some(v) = self.include_vector {
            q.set_include_vector(v)?;
        }
        if let Some(v) = self.include_doc_id {
            q.set_include_doc_id(v)?;
        }
        if let Some(fields) = self.output_fields {
            let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
            q.set_output_fields(&refs)?;
        }
        if let Some(bytes) = self.vector_bytes {
            q.set_query_vector_raw(&bytes)?;
        }
        if let Some(p) = self.params {
            match p {
                QueryParamsKind::Hnsw(p) => q.set_hnsw_params(p)?,
                QueryParamsKind::Ivf(p) => q.set_ivf_params(p)?,
                QueryParamsKind::Flat(p) => q.set_flat_params(p)?,
            }
        }
        Ok(q)
    }
}
