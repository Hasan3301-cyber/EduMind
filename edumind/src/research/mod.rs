//! Deterministic research discovery, analysis, validation, and run persistence.

pub mod analysis;
pub mod bibliography;
pub mod connectors;
pub mod fulltext;
pub mod ocr;
pub mod pipeline;
pub mod plugins;
pub mod project;
pub mod ranking;
pub mod run_store;
pub mod supervisor;
pub mod synthesis;
pub mod validation;

pub use bibliography::{
    BibliographyExport, BibliographyFormat, export_bibliography, to_bibtex, to_ris,
};
pub use connectors::{
    ArxivConnector, ConnectorRegistry, DiscoveryFailure, DiscoveryResult, LiteratureConnector,
    PubMedConnector, ScopusConnector, SemanticScholarConnector, StaticLiteratureConnector,
    WebFallbackConnector, deduplicate_papers,
};
pub use fulltext::{
    DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_TARGET, DeepAnswer, DeepPassage, EmbeddedTextChunk,
    FullTextChunk, FullTextDocument, FullTextIngestRequest, FullTextResearchService, FullTextStore,
    MAX_PDF_BYTES, NewFullTextDocument, StoredFullText, chunk_text, downloadable_url, embed_chunks,
    extract_pdf_text, fulltext_database_path, search_passages,
};
pub use ocr::{OcrMode, ocr_pdf, ocr_pdf_bytes};
pub use pipeline::{ResearchPipelineEngine, ResearchPipelineResult};
pub use plugins::TypedResearchPluginRegistry;
pub use project::{
    ProjectAnswer, ProjectAnswerSource, ProjectStore, ResearchProjectService, project_database_path,
};
pub use ranking::{rank_discovery_papers, rank_papers_for_query, semantic_rank_papers};
pub use run_store::{ResearchRunRecord, ResearchRunStore};
pub use supervisor::{
    CorpusHealth, GapProvenance, ReadingPlanItem, ResearchGapsReport, ResearchSupervision,
    ResearchSupervisorService, StatedGap, StatedGapKind, build_supervision, extract_stated_gaps,
};
pub use synthesis::{ComparisonRow, ResearchSynthesis, SynthesisSection, build_synthesis};
pub use validation::{build_literature_graph, evidence_from_papers, validate_claims};
