//! Shared Cerul contract types for app, API, agent, and future cloud surfaces.
//!
//! This crate intentionally avoids dependencies on implementation crates such
//! as `cerul-api`, `cerul-storage`, or `cerul-pipeline`. OpenAPI or TypeScript
//! generation should consume these serde-compatible shapes instead of scraping
//! storage structs or route-local DTOs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTRACT_VERSION: &str = "2026-07-03";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentModality {
    Video,
    Audio,
    Image,
    Document,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexItem {
    pub id: String,
    pub source_id: String,
    pub modality: ContentModality,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_path: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceLocator {
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalUnit {
    pub id: String,
    pub item_id: String,
    pub unit_kind: String,
    pub content_text: String,
    pub evidence: EvidenceLocator,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modalities: Vec<ContentModality>,
    #[serde(default)]
    pub include_evidence: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_locator_round_trips_document_pages() {
        let locator = EvidenceLocator {
            item_id: "item-doc".to_string(),
            chunk_id: Some("chunk-page-2".to_string()),
            start_sec: None,
            end_sec: None,
            page: Some(2),
            section: Some("Executive Summary".to_string()),
            frame_path: None,
            source_url: Some("cerul://items/item-doc?page=2".to_string()),
        };

        let json = serde_json::to_value(&locator).unwrap();
        assert_eq!(json["page"], 2);
        assert_eq!(json["section"], "Executive Summary");
        let restored: EvidenceLocator = serde_json::from_value(json).unwrap();
        assert_eq!(restored, locator);
    }

    #[test]
    fn search_request_serializes_as_agent_contract() {
        let request = SearchRequest {
            query: "find launch mentions".to_string(),
            limit: Some(5),
            modalities: vec![ContentModality::Video, ContentModality::Document],
            include_evidence: true,
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["query"], "find launch mentions");
        assert_eq!(json["limit"], 5);
        assert_eq!(json["modalities"], serde_json::json!(["video", "document"]));
        assert_eq!(json["include_evidence"], true);
    }

    #[test]
    fn retrieval_unit_carries_evidence_locator() {
        let unit = RetrievalUnit {
            id: "unit-1".to_string(),
            item_id: "item-video".to_string(),
            unit_kind: "transcript".to_string(),
            content_text: "The product launched in May.".to_string(),
            evidence: EvidenceLocator {
                item_id: "item-video".to_string(),
                chunk_id: Some("chunk-1".to_string()),
                start_sec: Some(12.0),
                end_sec: Some(18.0),
                page: None,
                section: None,
                frame_path: None,
                source_url: Some("cerul://items/item-video?t=12".to_string()),
            },
            metadata: serde_json::json!({ "speaker": "founder" }),
        };

        let json = serde_json::to_value(&unit).unwrap();
        assert_eq!(json["evidence"]["start_sec"], 12.0);
        assert_eq!(json["metadata"]["speaker"], "founder");
        let restored: RetrievalUnit = serde_json::from_value(json).unwrap();
        assert_eq!(restored, unit);
    }
}
