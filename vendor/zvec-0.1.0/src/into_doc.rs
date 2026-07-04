//! [`IntoDoc`] trait and its companion derive.
//!
//! The `derive` cargo feature enables `#[derive(IntoDoc)]`, which
//! emits an `IntoDoc` impl that reads fields off a user-owned struct
//! and writes them into a freshly-allocated [`Doc`]. Field-level
//! `#[zvec(...)]` attributes pick the primary key, override field
//! names, skip fields, and disambiguate collection types that the
//! derive can't map on its own (e.g. `Vec<f32>`).
//!
//! ```no_run
//! # #[cfg(feature = "derive")]
//! # fn main() -> zvec::Result<()> {
//! use zvec::{Doc, IntoDoc};
//!
//! #[derive(IntoDoc)]
//! struct Article {
//!     #[zvec(pk)]
//!     id: String,
//!     title: String,
//!     #[zvec(vector_fp32)]
//!     embedding: Vec<f32>,
//!     summary: Option<String>,
//! }
//!
//! let a = Article {
//!     id: "a".into(),
//!     title: "Hello".into(),
//!     embedding: vec![0.1, 0.2, 0.3],
//!     summary: None,
//! };
//! let _doc: Doc = a.into_doc()?;
//! # Ok(()) }
//! # #[cfg(not(feature = "derive"))]
//! # fn main() {}
//! ```

use crate::doc::Doc;
use crate::error::Result;

/// Convert `&self` into a freshly-allocated [`Doc`].
///
/// Implemented by the `#[derive(IntoDoc)]` macro from the `zvec-derive`
/// crate (enabled via the `derive` cargo feature). You can also
/// implement this manually if the derive's rules don't fit your shape.
///
/// The `into_` prefix here follows the pattern set by `serde::Serialize`
/// rather than the `Into` trait — the conversion is read-only (`&self`)
/// and leaves the source intact.
#[allow(clippy::wrong_self_convention)]
pub trait IntoDoc {
    fn into_doc(&self) -> Result<Doc>;
}
