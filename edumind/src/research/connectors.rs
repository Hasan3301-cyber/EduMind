use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::Utc;
use edumind_core::{LiteratureSource, PaperMetadata};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    infra::{EduMindError, Result},
    research::analysis::normalize_terms,
};

/// Native source adapter for one academic literature provider.
#[async_trait]
pub trait LiteratureConnector: Send + Sync {
    /// Stable source identifier exposed in API requests and persisted paper records.
    fn source(&self) -> LiteratureSource;

    /// Searches the provider and returns normalized paper metadata.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>>;
}

/// A non-fatal source failure captured while the remaining connectors continue discovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryFailure {
    pub source: LiteratureSource,
    pub message: String,
}

/// Combined result of a multi-source literature discovery request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryResult {
    #[serde(default)]
    pub papers: Vec<PaperMetadata>,
    #[serde(default)]
    pub failures: Vec<DiscoveryFailure>,
}

/// Ordered registry of literature connectors available to the research pipeline.
#[derive(Clone)]
pub struct ConnectorRegistry {
    connectors: BTreeMap<LiteratureSource, Arc<dyn LiteratureConnector>>,
}

impl ConnectorRegistry {
    /// Creates an empty registry for tests or explicitly isolated research sessions.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            connectors: BTreeMap::new(),
        }
    }

    /// Registers or replaces one source connector.
    pub fn register(&mut self, connector: Arc<dyn LiteratureConnector>) {
        self.connectors.insert(connector.source(), connector);
    }

    /// Returns known connector sources in deterministic source order.
    #[must_use]
    pub fn available_sources(&self) -> Vec<LiteratureSource> {
        self.connectors.keys().copied().collect()
    }

    /// Searches all requested connectors and preserves successful results if one source fails.
    pub async fn discover(
        &self,
        query: &str,
        requested_sources: &[LiteratureSource],
        limit: usize,
    ) -> DiscoveryResult {
        let mut result = DiscoveryResult::default();
        let sources = if requested_sources.is_empty() {
            self.available_sources()
        } else {
            requested_sources.to_vec()
        };
        let limit = limit.clamp(1, 100);
        for source in sources {
            let Some(connector) = self.connectors.get(&source) else {
                if source == LiteratureSource::Manual {
                    continue;
                }
                result.failures.push(DiscoveryFailure {
                    source,
                    message: "connector is not configured".to_owned(),
                });
                continue;
            };
            match connector.search(query, limit).await {
                Ok(mut papers) => result.papers.append(&mut papers),
                Err(error) => result.failures.push(DiscoveryFailure {
                    source,
                    message: error.to_string(),
                }),
            }
        }
        result.papers = deduplicate_papers(result.papers);
        result
    }
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        let client = Client::new();
        let mut registry = Self::empty();
        registry.register(Arc::new(SemanticScholarConnector::new(client.clone())));
        registry.register(Arc::new(PubMedConnector::new(client.clone())));
        registry.register(Arc::new(ArxivConnector::new(client.clone())));
        registry.register(Arc::new(ScopusConnector::from_environment(client.clone())));
        registry.register(Arc::new(WebFallbackConnector::new(client)));
        registry
    }
}

/// Deterministic in-memory connector used by tests and offline callers with seed data.
#[derive(Clone, Debug)]
pub struct StaticLiteratureConnector {
    source: LiteratureSource,
    papers: Vec<PaperMetadata>,
}

impl StaticLiteratureConnector {
    /// Creates a connector that always returns the supplied corpus in stable deduplicated order.
    #[must_use]
    pub fn new(source: LiteratureSource, papers: Vec<PaperMetadata>) -> Self {
        Self { source, papers }
    }
}

#[async_trait]
impl LiteratureConnector for StaticLiteratureConnector {
    fn source(&self) -> LiteratureSource {
        self.source
    }

    async fn search(&self, _query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        let mut papers = deduplicate_papers(self.papers.clone());
        papers.truncate(limit);
        Ok(papers)
    }
}

/// Semantic Scholar Graph API connector.
#[derive(Clone)]
pub struct SemanticScholarConnector {
    client: Client,
    endpoint: String,
}

impl SemanticScholarConnector {
    /// Creates a connector with the public Semantic Scholar search endpoint.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            endpoint: "https://api.semanticscholar.org/graph/v1/paper/search".to_owned(),
        }
    }
}

#[async_trait]
impl LiteratureConnector for SemanticScholarConnector {
    fn source(&self) -> LiteratureSource {
        LiteratureSource::SemanticScholar
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        require_query(query)?;
        let payload: Value = self
            .client
            .get(&self.endpoint)
            .query(&[
                ("query", query),
                ("limit", &limit.clamp(1, 100).to_string()),
                (
                    "fields",
                    "paperId,title,authors,abstract,year,venue,externalIds,citationCount,openAccessPdf,url,fieldsOfStudy,references.paperId,citations.paperId",
                ),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(payload
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(semantic_scholar_paper)
            .collect())
    }
}

/// NCBI PubMed E-utilities connector.
#[derive(Clone)]
pub struct PubMedConnector {
    client: Client,
    search_endpoint: String,
    summary_endpoint: String,
}

impl PubMedConnector {
    /// Creates a connector backed by the public PubMed JSON endpoints.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            search_endpoint: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi"
                .to_owned(),
            summary_endpoint: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi"
                .to_owned(),
        }
    }
}

#[async_trait]
impl LiteratureConnector for PubMedConnector {
    fn source(&self) -> LiteratureSource {
        LiteratureSource::PubMed
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        require_query(query)?;
        let requested_limit = limit.clamp(1, 100).to_string();
        let search: Value = self
            .client
            .get(&self.search_endpoint)
            .query(&[
                ("db", "pubmed"),
                ("retmode", "json"),
                ("term", query),
                ("retmax", requested_limit.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let ids = search
            .pointer("/esearchresult/idlist")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let joined_ids = ids.join(",");
        let summaries: Value = self
            .client
            .get(&self.summary_endpoint)
            .query(&[
                ("db", "pubmed"),
                ("retmode", "json"),
                ("id", joined_ids.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let result = summaries.get("result").unwrap_or(&Value::Null);
        Ok(ids
            .iter()
            .filter_map(|id| result.get(id).and_then(|item| pubmed_paper(id, item)))
            .collect())
    }
}

/// arXiv Atom feed connector implemented without an XML dependency for offline-friendly builds.
#[derive(Clone)]
pub struct ArxivConnector {
    client: Client,
    endpoint: String,
}

impl ArxivConnector {
    /// Creates a connector backed by arXiv's public Atom endpoint.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            endpoint: "https://export.arxiv.org/api/query".to_owned(),
        }
    }
}

#[async_trait]
impl LiteratureConnector for ArxivConnector {
    fn source(&self) -> LiteratureSource {
        LiteratureSource::Arxiv
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        require_query(query)?;
        let search_query = format!("all:{query}");
        let start = "0";
        let max_results = limit.clamp(1, 100).to_string();
        let feed = self
            .client
            .get(&self.endpoint)
            .query(&[
                ("search_query", search_query.as_str()),
                ("start", start),
                ("max_results", max_results.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(xml_tag_blocks(&feed, "entry")
            .into_iter()
            .filter_map(arxiv_paper)
            .collect())
    }
}

/// Scopus search connector. It remains dormant until `EDUMIND_SCOPUS_API_KEY` is set.
#[derive(Clone)]
pub struct ScopusConnector {
    client: Client,
    endpoint: String,
    api_key: Option<String>,
}

impl ScopusConnector {
    /// Creates a connector using an explicitly supplied Scopus API key.
    #[must_use]
    pub fn new(client: Client, api_key: Option<String>) -> Self {
        Self {
            client,
            endpoint: "https://api.elsevier.com/content/search/scopus".to_owned(),
            api_key,
        }
    }

    /// Reads the optional Scopus API key from the process environment.
    #[must_use]
    pub fn from_environment(client: Client) -> Self {
        Self::new(client, std::env::var("EDUMIND_SCOPUS_API_KEY").ok())
    }
}

#[async_trait]
impl LiteratureConnector for ScopusConnector {
    fn source(&self) -> LiteratureSource {
        LiteratureSource::Scopus
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        require_query(query)?;
        let api_key = self
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                EduMindError::Research(
                    "Scopus discovery is unavailable because EDUMIND_SCOPUS_API_KEY is not set"
                        .to_owned(),
                )
            })?;
        let count = limit.clamp(1, 100).to_string();
        let payload: Value = self
            .client
            .get(&self.endpoint)
            .header("X-ELS-APIKey", api_key)
            .query(&[("query", query), ("count", count.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(payload
            .pointer("/search-results/entry")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(scopus_paper)
            .collect())
    }
}

/// Crossref-backed web fallback when dedicated academic sources are unavailable.
#[derive(Clone)]
pub struct WebFallbackConnector {
    client: Client,
    endpoint: String,
}

impl WebFallbackConnector {
    /// Creates the public Crossref fallback connector.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            endpoint: "https://api.crossref.org/works".to_owned(),
        }
    }
}

#[async_trait]
impl LiteratureConnector for WebFallbackConnector {
    fn source(&self) -> LiteratureSource {
        LiteratureSource::WebFallback
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<PaperMetadata>> {
        require_query(query)?;
        let rows = limit.clamp(1, 100).to_string();
        let payload: Value = self
            .client
            .get(&self.endpoint)
            .header("User-Agent", "EduMind research pipeline")
            .query(&[("query", query), ("rows", rows.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(payload
            .pointer("/message/items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(crossref_paper)
            .collect())
    }
}

/// Deduplicates paper records by DOI, arXiv ID, or normalized title while retaining richer fields.
#[must_use]
pub fn deduplicate_papers(papers: impl IntoIterator<Item = PaperMetadata>) -> Vec<PaperMetadata> {
    let mut unique = BTreeMap::<String, PaperMetadata>::new();
    for paper in papers {
        let key = paper_dedup_key(&paper);
        if let Some(existing) = unique.remove(&key) {
            unique.insert(key, merge_papers(existing, paper));
        } else {
            unique.insert(key, paper);
        }
    }
    unique.into_values().collect()
}

fn require_query(query: &str) -> Result<()> {
    if query.trim().is_empty() {
        return Err(EduMindError::Research(
            "literature discovery requires a non-empty query".to_owned(),
        ));
    }
    Ok(())
}

fn semantic_scholar_paper(item: &Value) -> Option<PaperMetadata> {
    let title = string_value(item.get("title"))?;
    let paper_id =
        string_value(item.get("paperId")).unwrap_or_else(|| normalized_identifier(&title));
    let mut source_ids = object_strings(item.get("externalIds"));
    source_ids.insert("semantic_scholar".to_owned(), paper_id.clone());
    Some(PaperMetadata {
        id: paper_id,
        title,
        authors: object_array_strings(item.get("authors"), "name"),
        abstract_text: string_value(item.get("abstract")).unwrap_or_default(),
        year: integer_value(item.get("year")),
        venue: string_value(item.get("venue")),
        doi: source_ids
            .get("DOI")
            .cloned()
            .or_else(|| source_ids.get("doi").cloned()),
        citation_count: unsigned_value(item.get("citationCount")),
        open_access_url: item
            .get("openAccessPdf")
            .and_then(|value| value.get("url"))
            .and_then(|value| string_value(Some(value))),
        source_url: string_value(item.get("url")),
        source: LiteratureSource::SemanticScholar,
        source_ids,
        keywords: Vec::new(),
        fields_of_study: string_array(item.get("fieldsOfStudy")),
        referenced_paper_ids: object_array_strings(item.get("references"), "paperId"),
        influenced_paper_ids: object_array_strings(item.get("citations"), "paperId"),
        fetched_at: Some(Utc::now()),
    })
}

fn pubmed_paper(pubmed_id: &str, item: &Value) -> Option<PaperMetadata> {
    let title = string_value(item.get("title"))?;
    let mut source_ids = BTreeMap::new();
    source_ids.insert("pubmed".to_owned(), pubmed_id.to_owned());
    let doi = item
        .get("articleids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|article_id| {
            article_id
                .get("idtype")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("doi"))
        })
        .and_then(|article_id| string_value(article_id.get("value")));
    if let Some(doi) = &doi {
        source_ids.insert("doi".to_owned(), doi.clone());
    }
    let date = string_value(item.get("pubdate")).or_else(|| string_value(item.get("epubdate")));
    Some(PaperMetadata {
        id: format!("pubmed:{pubmed_id}"),
        title,
        authors: object_array_strings(item.get("authors"), "name"),
        abstract_text: String::new(),
        year: date.as_deref().and_then(year_from_text),
        venue: string_value(item.get("fulljournalname"))
            .or_else(|| string_value(item.get("source"))),
        doi,
        citation_count: 0,
        open_access_url: None,
        source_url: Some(format!("https://pubmed.ncbi.nlm.nih.gov/{pubmed_id}/")),
        source: LiteratureSource::PubMed,
        source_ids,
        keywords: Vec::new(),
        fields_of_study: Vec::new(),
        referenced_paper_ids: Vec::new(),
        influenced_paper_ids: Vec::new(),
        fetched_at: Some(Utc::now()),
    })
}

fn arxiv_paper(entry: &str) -> Option<PaperMetadata> {
    let source_url = xml_tag_text(entry, "id")?;
    let arxiv_id = source_url.rsplit('/').next()?.trim().to_owned();
    let title = xml_tag_text(entry, "title")?;
    let mut source_ids = BTreeMap::new();
    source_ids.insert("arxiv".to_owned(), arxiv_id.clone());
    let doi = xml_tag_text(entry, "arxiv:doi");
    if let Some(doi) = &doi {
        source_ids.insert("doi".to_owned(), doi.clone());
    }
    let mut fields_of_study = xml_tag_attribute_values(entry, "category", "term");
    fields_of_study.extend(xml_tag_attribute_values(
        entry,
        "arxiv:primary_category",
        "term",
    ));
    fields_of_study.sort();
    fields_of_study.dedup();
    Some(PaperMetadata {
        id: format!("arxiv:{arxiv_id}"),
        title,
        authors: xml_tag_blocks(entry, "author")
            .into_iter()
            .filter_map(|author| xml_tag_text(author, "name"))
            .collect(),
        abstract_text: xml_tag_text(entry, "summary").unwrap_or_default(),
        year: xml_tag_text(entry, "published")
            .as_deref()
            .and_then(year_from_text),
        venue: Some("arXiv".to_owned()),
        doi,
        citation_count: 0,
        open_access_url: Some(source_url.clone()),
        source_url: Some(source_url),
        source: LiteratureSource::Arxiv,
        source_ids,
        keywords: Vec::new(),
        fields_of_study,
        referenced_paper_ids: Vec::new(),
        influenced_paper_ids: Vec::new(),
        fetched_at: Some(Utc::now()),
    })
}

fn scopus_paper(item: &Value) -> Option<PaperMetadata> {
    let title = string_value(item.get("dc:title"))?;
    let scopus_id = string_value(item.get("eid"))
        .or_else(|| string_value(item.get("dc:identifier")))
        .unwrap_or_else(|| normalized_identifier(&title));
    let doi = string_value(item.get("prism:doi"));
    let mut source_ids = BTreeMap::new();
    source_ids.insert("scopus".to_owned(), scopus_id.clone());
    if let Some(doi) = &doi {
        source_ids.insert("doi".to_owned(), doi.clone());
    }
    Some(PaperMetadata {
        id: scopus_id,
        title,
        authors: string_value(item.get("dc:creator")).into_iter().collect(),
        abstract_text: String::new(),
        year: string_value(item.get("prism:coverDate"))
            .as_deref()
            .and_then(year_from_text),
        venue: string_value(item.get("prism:publicationName")),
        doi,
        citation_count: string_value(item.get("citedby-count"))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
        open_access_url: string_value(item.get("prism:url")),
        source_url: string_value(item.get("prism:url")),
        source: LiteratureSource::Scopus,
        source_ids,
        keywords: Vec::new(),
        fields_of_study: Vec::new(),
        referenced_paper_ids: Vec::new(),
        influenced_paper_ids: Vec::new(),
        fetched_at: Some(Utc::now()),
    })
}

fn crossref_paper(item: &Value) -> Option<PaperMetadata> {
    let title = item
        .get("title")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|value| string_value(Some(value)))?;
    let doi = string_value(item.get("DOI"));
    let source_url = string_value(item.get("URL"))
        .or_else(|| doi.as_ref().map(|value| format!("https://doi.org/{value}")));
    let id = doi
        .clone()
        .or_else(|| source_url.clone())
        .unwrap_or_else(|| normalized_identifier(&title));
    let mut source_ids = BTreeMap::new();
    if let Some(doi) = &doi {
        source_ids.insert("doi".to_owned(), doi.clone());
    }
    source_ids.insert("crossref".to_owned(), id.clone());
    Some(PaperMetadata {
        id,
        title,
        authors: crossref_authors(item.get("author")),
        abstract_text: string_value(item.get("abstract"))
            .map(|value| strip_xml_tags(&value))
            .unwrap_or_default(),
        year: crossref_year(item),
        venue: item
            .get("container-title")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|value| string_value(Some(value))),
        doi,
        citation_count: unsigned_value(item.get("is-referenced-by-count")),
        open_access_url: source_url.clone(),
        source_url,
        source: LiteratureSource::WebFallback,
        source_ids,
        keywords: string_array(item.get("subject")),
        fields_of_study: Vec::new(),
        referenced_paper_ids: Vec::new(),
        influenced_paper_ids: Vec::new(),
        fetched_at: Some(Utc::now()),
    })
}

fn paper_dedup_key(paper: &PaperMetadata) -> String {
    if let Some(doi) = paper
        .doi
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return format!("doi:{}", normalized_identifier(doi));
    }
    if let Some(arxiv_id) = paper
        .source_ids
        .get("arxiv")
        .filter(|value| !value.trim().is_empty())
    {
        return format!("arxiv:{}", normalized_identifier(arxiv_id));
    }
    if paper.source == LiteratureSource::Arxiv && !paper.id.trim().is_empty() {
        return format!("arxiv:{}", normalized_identifier(&paper.id));
    }
    let title = normalized_title(&paper.title);
    if !title.is_empty() {
        return format!("title:{title}");
    }
    format!(
        "id:{}:{}",
        paper.source as u8,
        normalized_identifier(&paper.id)
    )
}

fn merge_papers(existing: PaperMetadata, candidate: PaperMetadata) -> PaperMetadata {
    let (mut primary, secondary) = if paper_richness(&candidate) > paper_richness(&existing) {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    primary.citation_count = primary.citation_count.max(secondary.citation_count);
    if primary.abstract_text.chars().count() < secondary.abstract_text.chars().count() {
        primary.abstract_text = secondary.abstract_text.clone();
    }
    if primary.year.is_none() {
        primary.year = secondary.year;
    }
    if primary.venue.is_none() {
        primary.venue = secondary.venue.clone();
    }
    if primary.doi.is_none() {
        primary.doi = secondary.doi.clone();
    }
    if primary.open_access_url.is_none() {
        primary.open_access_url = secondary.open_access_url.clone();
    }
    if primary.source_url.is_none() {
        primary.source_url = secondary.source_url.clone();
    }
    if secondary.fetched_at > primary.fetched_at {
        primary.fetched_at = secondary.fetched_at;
    }
    merge_strings(&mut primary.authors, secondary.authors);
    merge_strings(&mut primary.keywords, secondary.keywords);
    merge_strings(&mut primary.fields_of_study, secondary.fields_of_study);
    merge_strings(
        &mut primary.referenced_paper_ids,
        secondary.referenced_paper_ids,
    );
    merge_strings(
        &mut primary.influenced_paper_ids,
        secondary.influenced_paper_ids,
    );
    for (key, value) in secondary.source_ids {
        primary.source_ids.entry(key).or_insert(value);
    }
    primary
}

fn paper_richness(paper: &PaperMetadata) -> (u32, usize, i32, usize, String) {
    (
        paper.citation_count,
        paper.abstract_text.chars().count(),
        paper.year.unwrap_or(i32::MIN),
        paper.keywords.len() + paper.fields_of_study.len(),
        paper.id.clone(),
    )
}

fn merge_strings(target: &mut Vec<String>, values: Vec<String>) {
    let mut merged = target
        .iter()
        .chain(&values)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    *target = std::mem::take(&mut merged).into_iter().collect();
}

fn object_strings(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| string_value(Some(value)).map(|value| (key.clone(), value)))
        .collect()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| string_value(Some(item)))
        .collect()
}

fn object_array_strings(value: Option<&Value>, field: &str) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| string_value(item.get(field)))
        .collect()
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn integer_value(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn unsigned_value(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn crossref_authors(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|author| {
            let given = string_value(author.get("given")).unwrap_or_default();
            let family = string_value(author.get("family")).unwrap_or_default();
            let full_name = format!("{given} {family}").trim().to_owned();
            (!full_name.is_empty()).then_some(full_name)
        })
        .collect()
}

fn crossref_year(item: &Value) -> Option<i32> {
    ["published-print", "published-online", "issued", "created"]
        .into_iter()
        .find_map(|field| {
            item.get(field)
                .and_then(|value| value.get("date-parts"))
                .and_then(Value::as_array)
                .and_then(|parts| parts.first())
                .and_then(Value::as_array)
                .and_then(|parts| parts.first())
                .and_then(Value::as_i64)
                .and_then(|year| i32::try_from(year).ok())
        })
}

fn year_from_text(value: &str) -> Option<i32> {
    let digits = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    digits.get(..4)?.parse().ok()
}

fn normalized_identifier(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .to_ascii_lowercase()
}

fn normalized_title(value: &str) -> String {
    normalize_terms(value).join(" ")
}

fn xml_tag_blocks<'a>(value: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut cursor = 0;
    let mut blocks = Vec::new();
    while let Some(relative_start) = value[cursor..].find(&open) {
        let start = cursor + relative_start;
        let Some(relative_open_end) = value[start..].find('>') else {
            break;
        };
        let content_start = start + relative_open_end + 1;
        let Some(relative_end) = value[content_start..].find(&close) else {
            break;
        };
        let end = content_start + relative_end + close.len();
        blocks.push(&value[start..end]);
        cursor = end;
    }
    blocks
}

fn xml_tag_text(value: &str, tag: &str) -> Option<String> {
    xml_tag_blocks(value, tag)
        .into_iter()
        .next()
        .and_then(|block| block.find('>').map(|index| &block[index + 1..]))
        .and_then(|content| content.rfind('<').map(|index| &content[..index]))
        .map(strip_xml_tags)
        .filter(|text| !text.is_empty())
}

fn xml_tag_attribute_values(value: &str, tag: &str, attribute: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let needle = format!("{attribute}=\"");
    let mut cursor = 0;
    let mut values = Vec::new();
    while let Some(relative_start) = value[cursor..].find(&open) {
        let start = cursor + relative_start;
        let Some(relative_end) = value[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let fragment = &value[start..end];
        if let Some(attribute_start) = fragment.find(&needle) {
            let value_start = attribute_start + needle.len();
            if let Some(value_end) = fragment[value_start..].find('"') {
                let candidate = &fragment[value_start..value_start + value_end];
                if !candidate.trim().is_empty() {
                    values.push(candidate.trim().to_owned());
                }
            }
        }
        cursor = end;
    }
    values
}

fn strip_xml_tags(value: &str) -> String {
    let mut plain = String::new();
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => plain.push(character),
            _ => {}
        }
    }
    plain
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use edumind_core::{LiteratureSource, PaperMetadata};

    use super::{
        ConnectorRegistry, LiteratureConnector, StaticLiteratureConnector, arxiv_paper,
        deduplicate_papers,
    };

    #[test]
    fn deduplication_prefers_richer_metadata_and_merges_sources() {
        let first = PaperMetadata {
            id: "source-a".to_owned(),
            title: "Retrieval for education".to_owned(),
            doi: Some("10.1/example".to_owned()),
            keywords: vec!["retrieval".to_owned()],
            ..PaperMetadata::default()
        };
        let second = PaperMetadata {
            id: "source-b".to_owned(),
            title: "Retrieval for education".to_owned(),
            doi: Some("10.1/example".to_owned()),
            abstract_text: "A much richer abstract.".to_owned(),
            citation_count: 8,
            fields_of_study: vec!["education".to_owned()],
            ..PaperMetadata::default()
        };

        let papers = deduplicate_papers(vec![first, second]);

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].citation_count, 8);
        assert_eq!(papers[0].keywords, vec!["retrieval"]);
        assert_eq!(papers[0].fields_of_study, vec!["education"]);
    }

    #[tokio::test]
    async fn registry_keeps_successful_static_sources_when_another_is_missing() {
        let paper = PaperMetadata {
            id: "paper".to_owned(),
            title: "Static corpus".to_owned(),
            ..PaperMetadata::default()
        };
        let mut registry = ConnectorRegistry::empty();
        let connector: Arc<dyn LiteratureConnector> = Arc::new(StaticLiteratureConnector::new(
            LiteratureSource::Manual,
            vec![paper],
        ));
        registry.register(connector);

        let result = registry
            .discover(
                "static",
                &[LiteratureSource::Manual, LiteratureSource::PubMed],
                10,
            )
            .await;

        assert_eq!(result.papers.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].source, LiteratureSource::PubMed);
    }

    #[test]
    fn parses_minimal_arxiv_atom_entry() {
        let entry = r#"<entry><id>http://arxiv.org/abs/2607.00001</id><title>Graph Learning</title><summary>Useful summary</summary><published>2026-07-01T00:00:00Z</published><author><name>Alex</name></author><category term="cs.AI"/></entry>"#;

        let paper = arxiv_paper(entry).unwrap();

        assert_eq!(paper.id, "arxiv:2607.00001");
        assert_eq!(paper.authors, vec!["Alex"]);
        assert_eq!(paper.fields_of_study, vec!["cs.AI"]);
    }
}
