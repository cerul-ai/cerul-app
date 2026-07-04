//! [`FromDoc`] trait and its companion derive.
//!
//! The inverse of [`crate::IntoDoc`]: given a [`DocRef`] returned from a
//! query or fetch, reconstruct a user struct. The `derive` cargo
//! feature enables `#[derive(FromDoc)]`, which reads fields off the
//! doc by name (respecting `#[zvec(pk | rename | skip | binary |
//! vector_fp32 | ...)]` attributes) and assembles the struct.
//!
//! ```no_run
//! # #[cfg(feature = "derive")]
//! # fn main() -> zvec::Result<()> {
//! use zvec::{FromDoc, IntoDoc};
//!
//! #[derive(IntoDoc, FromDoc)]
//! struct Article {
//!     #[zvec(pk)]                id: String,
//!     title: String,
//!     #[zvec(vector_fp32)]       embedding: Vec<f32>,
//!     summary: Option<String>,
//! }
//!
//! // Round-trip: struct → doc → insert → query → struct.
//! # let collection: zvec::Collection = unreachable!();
//! # let q: zvec::VectorQuery = unreachable!();
//! let results = collection.query(&q)?;
//! let articles: Vec<Article> =
//!     results.iter().map(Article::from_doc).collect::<zvec::Result<_>>()?;
//! # Ok(()) }
//! # #[cfg(not(feature = "derive"))]
//! # fn main() {}
//! ```

use crate::doc::DocRef;
use crate::error::Result;

/// Reconstruct `Self` from a [`DocRef`].
///
/// Implemented by the `#[derive(FromDoc)]` macro from the `zvec-derive`
/// crate (enabled via the `derive` cargo feature). The derive:
///
/// - reads scalar fields via the matching `DocRef::get_*` method,
/// - treats `#[zvec(pk)]` fields as the doc's primary key (via
///   [`DocRef::pk_copy`]),
/// - leaves `#[zvec(skip)]` fields initialised by
///   [`Default::default`],
/// - tolerates missing fields that are `Option<T>` (returns `None`),
/// - but errors on missing fields that are not `Option`.
///
/// Implement this manually if the derive's rules don't fit.
pub trait FromDoc: Sized {
    fn from_doc(doc: DocRef<'_>) -> Result<Self>;
}
