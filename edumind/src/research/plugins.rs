use std::{any::Any, collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use edumind_core::{
    Hypothesis, InsightType, LiteratureGraphRequest, PipelineContext, PluginInfo, PluginMetadata,
    PluginOutput, PluginPriority, PluginStage, PluginStatus, ResearchInsight, ResearchPlugin,
    ValidationReport,
};
use serde_json::json;

use crate::{
    infra::{EduMindError, Result},
    research::{
        analysis::{
            corpus_keyword_frequencies, corpus_novelty, detect_gaps, normalize_terms,
            paper_concepts, paper_terms, recency_share, truncate_chars,
        },
        build_literature_graph,
        connectors::{ConnectorRegistry, deduplicate_papers},
        evidence_from_papers,
        ranking::rank_discovery_papers,
        validate_claims,
    },
};

/// Typed, deterministic registry for native research pipeline plugins.
#[derive(Clone, Default)]
pub struct TypedResearchPluginRegistry {
    plugins: BTreeMap<String, Arc<dyn ResearchPlugin>>,
}

impl TypedResearchPluginRegistry {
    /// Builds the complete native plugin set required by the Phase 9 research pipeline.
    pub fn standard(connectors: ConnectorRegistry) -> Result<Self> {
        let mut registry = Self::default();
        registry.register(OrchestratorPlugin)?;
        registry.register(LiteratureDiscoveryPlugin::new(connectors))?;
        registry.register(PaperRankingPlugin)?;
        registry.register(KnowledgeGraphPlugin)?;
        registry.register(InsightGeneratorPlugin)?;
        registry.register(HypothesisEnginePlugin)?;
        registry.register(CriticPlugin)?;
        Ok(registry)
    }

    /// Registers a plugin and rejects duplicate or blank stable names.
    pub fn register<P>(&mut self, plugin: P) -> Result<()>
    where
        P: ResearchPlugin + 'static,
    {
        self.register_arc(Arc::new(plugin))
    }

    /// Registers a dynamically owned plugin implementation.
    pub fn register_arc(&mut self, plugin: Arc<dyn ResearchPlugin>) -> Result<()> {
        let name = plugin.name().trim();
        if name.is_empty() {
            return Err(EduMindError::Research(
                "research plugins require a non-empty name".to_owned(),
            ));
        }
        if self.plugins.contains_key(name) {
            return Err(EduMindError::Research(format!(
                "research plugin `{name}` is already registered"
            )));
        }
        self.plugins.insert(name.to_owned(), plugin);
        Ok(())
    }

    /// Returns native plugin metadata in execution order.
    #[must_use]
    pub fn infos(&self) -> Vec<PluginInfo> {
        self.ordered()
            .into_iter()
            .map(|plugin| PluginInfo {
                name: plugin.name().to_owned(),
                priority: plugin.priority(),
                stage: plugin.stage(),
                description: plugin.description().to_owned(),
                status: PluginStatus::Ready,
                metadata: PluginMetadata::native(plugin.name()),
                dependencies: plugin
                    .dependencies()
                    .iter()
                    .map(|dependency| (*dependency).to_owned())
                    .collect(),
            })
            .collect()
    }

    /// Returns plugins sorted by stage, priority, and stable name.
    #[must_use]
    pub(crate) fn ordered(&self) -> Vec<Arc<dyn ResearchPlugin>> {
        let mut plugins = self.plugins.values().cloned().collect::<Vec<_>>();
        plugins.sort_by(|left, right| {
            left.stage()
                .start()
                .cmp(&right.stage().start())
                .then_with(|| right.priority().rank().cmp(&left.priority().rank()))
                .then_with(|| left.name().cmp(right.name()))
        });
        plugins
    }
}

/// Normalizes the initial context and guarantees a usable research query.
#[derive(Clone, Debug, Default)]
pub struct OrchestratorPlugin;

#[async_trait]
impl ResearchPlugin for OrchestratorPlugin {
    fn name(&self) -> &'static str {
        "orchestrator-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::Critical
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(0)
    }

    fn description(&self) -> &'static str {
        "Normalizes the pipeline manifest, query, and seeded corpus."
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        context.topic = context.topic.trim().to_owned();
        context.query = context.query.trim().to_owned();
        if context.topic.is_empty() || context.query.is_empty() {
            return Err("research runs require a non-empty topic and query".to_owned());
        }
        context.max_results = context.max_results.clamp(1, 100);
        context.query_terms = normalize_terms(&context.query);
        context.papers = deduplicate_papers(std::mem::take(&mut context.papers));
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary: format!(
                "Normalized `{}` into {} query terms and {} seeded papers.",
                context.topic,
                context.query_terms.len(),
                context.papers.len()
            ),
            data: json!({
                "topic": context.topic,
                "query": context.query,
                "query_terms": context.query_terms,
                "seeded_papers": context.papers.len(),
            }),
            warnings: Vec::new(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Discovers literature through the configured native connector registry.
#[derive(Clone)]
pub struct LiteratureDiscoveryPlugin {
    connectors: ConnectorRegistry,
}

impl LiteratureDiscoveryPlugin {
    /// Builds a discovery plugin using caller-configured source connectors.
    #[must_use]
    pub fn new(connectors: ConnectorRegistry) -> Self {
        Self { connectors }
    }
}

#[async_trait]
impl ResearchPlugin for LiteratureDiscoveryPlugin {
    fn name(&self) -> &'static str {
        "literature-discovery-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::High
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(10)
    }

    fn description(&self) -> &'static str {
        "Queries Semantic Scholar, PubMed, arXiv, Scopus, and the web fallback."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["orchestrator-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        let seeded_papers = context.papers.len();
        let discovery = self
            .connectors
            .discover(
                &context.query,
                &context.requested_sources,
                context.max_results,
            )
            .await;
        let warnings = discovery
            .failures
            .iter()
            .map(|failure| format!("{:?}: {}", failure.source, failure.message))
            .collect::<Vec<_>>();
        context.warnings.extend(warnings.clone());
        context.papers.extend(discovery.papers);
        context.papers = deduplicate_papers(std::mem::take(&mut context.papers));
        let discovered_papers = context.papers.len().saturating_sub(seeded_papers);
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary: format!(
                "Retained {} unique papers after adding {} discovered records.",
                context.papers.len(),
                discovered_papers
            ),
            data: json!({
                "seeded_papers": seeded_papers,
                "discovered_papers": discovered_papers,
                "total_papers": context.papers.len(),
                "failed_sources": discovery.failures,
            }),
            warnings,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Ranks discovered papers without requiring a model or remote embedding service.
#[derive(Clone, Debug, Default)]
pub struct PaperRankingPlugin;

#[async_trait]
impl ResearchPlugin for PaperRankingPlugin {
    fn name(&self) -> &'static str {
        "paper-ranking-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::High
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(20)
    }

    fn description(&self) -> &'static str {
        "Ranks literature by query relevance, citations, recency, and abstract richness."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["literature-discovery-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        context.ranked_papers = rank_discovery_papers(&context.papers, &context.query);
        context.papers = context
            .ranked_papers
            .iter()
            .map(|ranked| ranked.paper.clone())
            .collect();
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary: format!(
                "Ranked {} papers for `{}`.",
                context.papers.len(),
                context.query
            ),
            data: json!({
                "ranking": context.ranked_papers.iter().map(|ranked| json!({
                    "paper_id": ranked.paper.id,
                    "score": ranked.score,
                })).collect::<Vec<_>>(),
            }),
            warnings: Vec::new(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builds the literature graph before higher-level insight generation.
#[derive(Clone, Debug, Default)]
pub struct KnowledgeGraphPlugin;

#[async_trait]
impl ResearchPlugin for KnowledgeGraphPlugin {
    fn name(&self) -> &'static str {
        "knowledge-graph-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::Normal
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(30)
    }

    fn description(&self) -> &'static str {
        "Builds citation and concept-similarity literature communities."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["paper-ranking-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        let graph = build_literature_graph(&LiteratureGraphRequest {
            papers: context.papers.clone(),
            similarity_threshold: 0.20,
        });
        let summary = format!(
            "Built a literature graph with {} nodes, {} edges, and {} communities.",
            graph.nodes.len(),
            graph.edges.len(),
            graph.communities.len()
        );
        let data = json!({
            "nodes": graph.nodes.len(),
            "edges": graph.edges.len(),
            "communities": graph.communities.len(),
        });
        context.graph = Some(graph);
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary,
            data,
            warnings: Vec::new(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Generates corpus-derived insights from measurable themes, recency, novelty, and gaps.
#[derive(Clone, Debug, Default)]
pub struct InsightGeneratorPlugin;

#[async_trait]
impl ResearchPlugin for InsightGeneratorPlugin {
    fn name(&self) -> &'static str {
        "insight-generator-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::Normal
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(40)
    }

    fn description(&self) -> &'static str {
        "Computes evidence-linked themes, maturity, novelty, and under-covered terms."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["paper-ranking-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        let papers = &context.papers;
        if papers.is_empty() {
            let warning =
                "No papers were available for deterministic insight generation.".to_owned();
            context.warnings.push(warning.clone());
            return Ok(PluginOutput {
                plugin_name: self.name().to_owned(),
                summary: warning.clone(),
                data: json!({"insight_count": 0}),
                warnings: vec![warning],
            });
        }
        let frequencies = corpus_keyword_frequencies(papers);
        let top_concepts = sorted_concepts(&frequencies, true)
            .into_iter()
            .take(3)
            .collect::<Vec<_>>();
        let mut insights = Vec::new();
        if let Some((leading_concept, leading_count)) = top_concepts.first() {
            let themes = top_concepts
                .iter()
                .map(|(concept, count)| format!("{concept} ({count}/{})", papers.len()))
                .collect::<Vec<_>>();
            let evidence_paper_ids = papers
                .iter()
                .filter(|paper| paper_concepts(paper).contains(leading_concept))
                .map(|paper| paper.id.clone())
                .collect();
            insights.push(ResearchInsight {
                id: format!("dominant-theme:{}", slug(leading_concept)),
                insight_type: InsightType::DominantTheme,
                title: format!("Dominant themes: {}", themes.join(", ")),
                summary: format!(
                    "`{leading_concept}` appears in {leading_count} of {} papers, with the corpus also clustering around {}.",
                    papers.len(),
                    themes.join(", ")
                ),
                evidence_paper_ids,
                confidence: (*leading_count as f64 / papers.len() as f64 * 0.60 + 0.30)
                    .clamp(0.0, 0.95),
                created_at: Some(context.started_at),
            });
        }
        let dated_papers = papers.iter().filter(|paper| paper.year.is_some()).count();
        if dated_papers > 0 {
            let recent_share = recency_share(papers, 3);
            let (insight_type, maturity) = if recent_share >= 0.5 {
                (InsightType::EmergingTrend, "emerging")
            } else {
                (InsightType::MaturingField, "maturing")
            };
            insights.push(ResearchInsight {
                id: format!("corpus-maturity:{maturity}"),
                insight_type,
                title: format!("Corpus appears {maturity}"),
                summary: format!(
                    "{:.0}% of the {dated_papers} dated papers fall within the latest three years represented by this corpus.",
                    recent_share * 100.0
                ),
                evidence_paper_ids: papers
                    .iter()
                    .filter(|paper| paper.year.is_some())
                    .map(|paper| paper.id.clone())
                    .collect(),
                confidence: (0.45 + recent_share * 0.45).clamp(0.0, 0.95),
                created_at: Some(context.started_at),
            });
        }
        let novelty = corpus_novelty(papers);
        insights.push(ResearchInsight {
            id: "corpus-novelty".to_owned(),
            insight_type: InsightType::CorpusNovelty,
            title: format!("Corpus novelty score: {:.0}%", novelty * 100.0),
            summary: format!(
                "The score combines the corpus-relative three-year share with one-off concept diversity across {} papers.",
                papers.len()
            ),
            evidence_paper_ids: papers.iter().map(|paper| paper.id.clone()).collect(),
            confidence: (0.35 + novelty * 0.55).clamp(0.0, 0.90),
            created_at: Some(context.started_at),
        });
        for gap in detect_gaps(&context.query_terms, papers)
            .into_iter()
            .take(3)
        {
            let covered_by = papers
                .iter()
                .filter(|paper| paper_terms(paper).contains(&gap))
                .map(|paper| paper.id.clone())
                .collect::<Vec<_>>();
            insights.push(ResearchInsight {
                id: format!("research-gap:{}", slug(&gap)),
                insight_type: InsightType::ResearchGap,
                title: format!("Under-covered query term: {gap}"),
                summary: format!(
                    "`{gap}` appears in {} of {} papers, making it a candidate gap for the stated query.",
                    covered_by.len(),
                    papers.len()
                ),
                evidence_paper_ids: covered_by,
                confidence: (0.60 + novelty * 0.25).clamp(0.0, 0.95),
                created_at: Some(context.started_at),
            });
        }
        context.insights = insights;
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary: format!(
                "Generated {} deterministic corpus insights from {} ranked papers.",
                context.insights.len(),
                papers.len()
            ),
            data: json!({
                "insight_count": context.insights.len(),
                "top_concepts": top_concepts,
                "novelty": novelty,
            }),
            warnings: Vec::new(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Creates testable hypotheses from detected coverage gaps and corpus novelty.
#[derive(Clone, Debug, Default)]
pub struct HypothesisEnginePlugin;

#[async_trait]
impl ResearchPlugin for HypothesisEnginePlugin {
    fn name(&self) -> &'static str {
        "hypothesis-engine-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::Normal
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(50)
    }

    fn description(&self) -> &'static str {
        "Produces testable hypotheses linked to corpus evidence and missing coverage."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["insight-generator-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        if context.papers.is_empty() {
            let warning =
                "No literature evidence was available for hypothesis generation.".to_owned();
            context.warnings.push(warning.clone());
            return Ok(PluginOutput {
                plugin_name: self.name().to_owned(),
                summary: warning.clone(),
                data: json!({"hypothesis_count": 0}),
                warnings: vec![warning],
            });
        }
        let frequencies = corpus_keyword_frequencies(&context.papers);
        let gaps = detect_gaps(&context.query_terms, &context.papers);
        let fallback_concepts = sorted_concepts(&frequencies, false)
            .into_iter()
            .map(|(concept, _)| concept)
            .collect::<Vec<_>>();
        let candidates = if gaps.is_empty() {
            fallback_concepts
        } else {
            gaps
        };
        let topic = truncate_chars(&context.topic, 120);
        let novelty = corpus_novelty(&context.papers);
        let evidence_paper_ids = context
            .ranked_papers
            .iter()
            .take(3)
            .map(|ranked| ranked.paper.id.clone())
            .collect::<Vec<_>>();
        context.hypotheses = candidates
            .into_iter()
            .take(3)
            .enumerate()
            .map(|(index, candidate)| {
                let coverage = context
                    .papers
                    .iter()
                    .filter(|paper| paper_terms(paper).contains(&candidate))
                    .count();
                let coverage_share = coverage as f64 / context.papers.len() as f64;
                Hypothesis {
                    id: format!("hypothesis-{}-{}", index + 1, slug(&candidate)),
                    statement: format!(
                        "For {topic}, explicitly evaluating `{candidate}` will reveal outcome differences that the current corpus cannot estimate reliably."
                    ),
                    rationale: format!(
                        "`{candidate}` is represented by {coverage} of {} papers; the corpus novelty score is {:.0}%.",
                        context.papers.len(),
                        novelty * 100.0
                    ),
                    test_method: format!(
                        "Run a preregistered comparison that varies `{candidate}`, holds the dominant study conditions constant, and measures a shared outcome."
                    ),
                    evidence_paper_ids: evidence_paper_ids.clone(),
                    confidence: (0.35 + (1.0 - coverage_share) * 0.40 + novelty * 0.20)
                        .clamp(0.0, 0.90),
                }
            })
            .collect();
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary: format!(
                "Generated {} testable hypotheses from corpus coverage patterns.",
                context.hypotheses.len()
            ),
            data: json!({"hypothesis_count": context.hypotheses.len()}),
            warnings: Vec::new(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Critiques supplied draft claims against the discovered evidence corpus.
#[derive(Clone, Debug, Default)]
pub struct CriticPlugin;

#[async_trait]
impl ResearchPlugin for CriticPlugin {
    fn name(&self) -> &'static str {
        "critic-plugin"
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::Low
    }

    fn stage(&self) -> PluginStage {
        PluginStage::single(60)
    }

    fn description(&self) -> &'static str {
        "Validates draft claims, citations, logical overreach, and bias against evidence."
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["literature-discovery-plugin"]
    }

    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String> {
        let no_claims = context.claims.is_empty();
        let report: ValidationReport = validate_claims(&edumind_core::ClaimValidationRequest {
            claims: context.claims.clone(),
            evidence: evidence_from_papers(&context.papers),
            support_threshold: 0.35,
        });
        if no_claims {
            context.warnings.push(
                "No draft claims were supplied, so the critic only recorded an empty validation report."
                    .to_owned(),
            );
        }
        let warning = no_claims.then(|| "No draft claims were supplied.".to_owned());
        let data = json!({
            "claim_count": report.claims.len(),
            "overall_score": report.overall_score,
            "hallucinations": report.hallucinations.len(),
            "citation_errors": report.citation_errors.len(),
            "logical_issues": report.logical_issues.len(),
            "bias_flags": report.bias_flags.len(),
        });
        let summary = format!(
            "Validated {} claims with an overall evidence score of {:.0}%.",
            report.claims.len(),
            report.overall_score * 100.0
        );
        context.validation = Some(report);
        Ok(PluginOutput {
            plugin_name: self.name().to_owned(),
            summary,
            data,
            warnings: warning.into_iter().collect(),
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn sorted_concepts(
    frequencies: &BTreeMap<String, usize>,
    descending: bool,
) -> Vec<(String, usize)> {
    let mut concepts = frequencies
        .iter()
        .map(|(concept, count)| (concept.clone(), *count))
        .collect::<Vec<_>>();
    concepts.sort_by(|left, right| {
        let counts = if descending {
            right.1.cmp(&left.1)
        } else {
            left.1.cmp(&right.1)
        };
        counts.then_with(|| left.0.cmp(&right.0))
    });
    concepts
}

fn slug(value: &str) -> String {
    let mut words = normalize_terms(value);
    if words.is_empty() {
        return "unspecified".to_owned();
    }
    words.truncate(4);
    words.join("-")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use edumind_core::{LiteratureSource, PaperMetadata, ResearchPlugin, ResearchRequest};

    use super::{LiteratureDiscoveryPlugin, TypedResearchPluginRegistry};
    use crate::research::{ConnectorRegistry, LiteratureConnector, StaticLiteratureConnector};

    #[test]
    fn native_registry_contains_every_required_plugin_in_stage_order() {
        let registry = TypedResearchPluginRegistry::standard(ConnectorRegistry::empty()).unwrap();
        let names = registry
            .infos()
            .into_iter()
            .map(|plugin| plugin.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "orchestrator-plugin",
                "literature-discovery-plugin",
                "paper-ranking-plugin",
                "knowledge-graph-plugin",
                "insight-generator-plugin",
                "hypothesis-engine-plugin",
                "critic-plugin",
            ]
        );
    }

    #[tokio::test]
    async fn discovery_plugin_merges_static_connector_results() {
        let mut connectors = ConnectorRegistry::empty();
        let connector: Arc<dyn LiteratureConnector> = Arc::new(StaticLiteratureConnector::new(
            LiteratureSource::Manual,
            vec![PaperMetadata {
                id: "paper".to_owned(),
                title: "Retrieval study".to_owned(),
                ..PaperMetadata::default()
            }],
        ));
        connectors.register(connector);
        let plugin = LiteratureDiscoveryPlugin::new(connectors);
        let mut request = ResearchRequest::new("retrieval");
        request.sources = vec![LiteratureSource::Manual];
        let mut context = edumind_core::PipelineContext::new(
            edumind_core::PipelineRunId::new(),
            request,
            chrono::Utc::now(),
        );

        let output = plugin.execute(&mut context).await.unwrap();

        assert_eq!(context.papers.len(), 1);
        assert!(output.summary.contains("1 unique papers"));
    }
}
