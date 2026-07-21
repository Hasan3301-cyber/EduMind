use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use edumind_core::{
    PipelineContext, PipelineEvent, PipelineProgress, PipelineRun, PipelineRunStatus,
    PipelineStage, PipelineStageStatus, PluginInfo, ResearchProject, ResearchRequest, TaskId,
};
use serde::{Deserialize, Serialize};

use crate::{
    infra::{EduMindError, Result},
    research::{
        ConnectorRegistry, ProjectStore, ResearchRunRecord, ResearchRunStore,
        TypedResearchPluginRegistry,
    },
};

/// Result returned after a focused research pipeline reaches a terminal state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchPipelineResult {
    pub run: PipelineRun,
    pub context: PipelineContext,
    pub project: ResearchProject,
}

/// Executes native research plugins in deterministic stage order and persists every transition.
#[derive(Clone)]
pub struct ResearchPipelineEngine {
    registry: TypedResearchPluginRegistry,
    run_store: ResearchRunStore,
    project_store: ProjectStore,
}

impl ResearchPipelineEngine {
    /// Builds the standard pipeline using live academic connectors and the supplied durable run store.
    pub fn new(run_store: ResearchRunStore, project_store: ProjectStore) -> Result<Self> {
        let registry = TypedResearchPluginRegistry::standard(ConnectorRegistry::default())?;
        Ok(Self::with_registry(run_store, project_store, registry))
    }

    /// Builds an engine around a caller-provided registry, primarily for deterministic tests.
    #[must_use]
    pub fn with_registry(
        run_store: ResearchRunStore,
        project_store: ProjectStore,
        registry: TypedResearchPluginRegistry,
    ) -> Self {
        Self {
            registry,
            run_store,
            project_store,
        }
    }

    /// Lists the native plugins exposed by this engine in execution order.
    #[must_use]
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.registry.infos()
    }

    /// Returns the durable run repository used by this engine.
    #[must_use]
    pub fn run_store(&self) -> ResearchRunStore {
        self.run_store.clone()
    }

    /// Returns the durable project store that receives completed pipeline corpora.
    #[must_use]
    pub fn project_store(&self) -> ProjectStore {
        self.project_store.clone()
    }

    /// Retrieves a persisted run snapshot by ID.
    pub fn get_run(
        &self,
        run_id: edumind_core::PipelineRunId,
    ) -> Result<Option<ResearchRunRecord>> {
        self.run_store.get(run_id)
    }

    /// Executes the complete focused research pipeline at a caller-supplied timestamp.
    pub async fn run(
        &self,
        request: ResearchRequest,
        started_at: DateTime<Utc>,
    ) -> Result<ResearchPipelineResult> {
        if request.topic.trim().is_empty() || request.effective_query().trim().is_empty() {
            return Err(EduMindError::Research(
                "research runs require a non-empty topic or query".to_owned(),
            ));
        }
        let plugins = self.registry.ordered();
        if plugins.is_empty() {
            return Err(EduMindError::Research(
                "research pipeline has no registered plugins".to_owned(),
            ));
        }
        let mut run = PipelineRun::new(TaskId::new(), started_at);
        run.status = PipelineRunStatus::Running;
        for plugin in &plugins {
            run.add_stage(PipelineStage::pending(plugin.name()), started_at);
        }
        let mut context = PipelineContext::new(run.id, request, started_at);
        self.run_store.save(&run, &context, None)?;

        let mut completed_plugins = BTreeSet::new();
        let total_stages = plugins.len();
        for (stage_index, plugin) in plugins.iter().enumerate() {
            let started = next_event_time(started_at, &context);
            let unmet_dependencies = plugin
                .dependencies()
                .iter()
                .filter(|dependency| !completed_plugins.contains(**dependency))
                .copied()
                .collect::<Vec<_>>();
            if !unmet_dependencies.is_empty() {
                let message = format!(
                    "plugin `{}` cannot run before dependencies: {}",
                    plugin.name(),
                    unmet_dependencies.join(", ")
                );
                self.fail_stage(
                    &mut run,
                    &mut context,
                    stage_index,
                    plugin.name(),
                    total_stages,
                    message.clone(),
                    started,
                )?;
                return Err(EduMindError::Research(message));
            }

            run.stages[stage_index].start(started);
            run.updated_at = started;
            self.record_progress(
                &run,
                &mut context,
                plugin.name(),
                stage_index,
                total_stages,
                format!("Started {}.", plugin.name()),
                None,
                started,
                None,
            )?;

            if let Err(error) = plugin.initialize().await {
                let message = format!("plugin `{}` failed to initialize: {error}", plugin.name());
                let failed_at = next_event_time(started_at, &context);
                self.fail_stage(
                    &mut run,
                    &mut context,
                    stage_index,
                    plugin.name(),
                    total_stages,
                    message.clone(),
                    failed_at,
                )?;
                return Err(EduMindError::Research(message));
            }

            if !plugin.should_run(&context) {
                let skipped_at = next_event_time(started_at, &context);
                run.stages[stage_index].status = PipelineStageStatus::Skipped;
                run.stages[stage_index].detail =
                    Some("Plugin declared itself inapplicable.".to_owned());
                run.stages[stage_index].completed_at = Some(skipped_at);
                run.updated_at = skipped_at;
                completed_plugins.insert(plugin.name());
                self.record_progress(
                    &run,
                    &mut context,
                    plugin.name(),
                    stage_index + 1,
                    total_stages,
                    format!("Skipped {}.", plugin.name()),
                    None,
                    skipped_at,
                    None,
                )?;
                continue;
            }

            match plugin.execute(&mut context).await {
                Ok(output) => {
                    let completed_at = next_event_time(started_at, &context);
                    run.stages[stage_index].complete(Some(output.summary.clone()), completed_at);
                    run.updated_at = completed_at;
                    completed_plugins.insert(plugin.name());
                    context.plugin_outputs.push(output.clone());
                    self.record_progress(
                        &run,
                        &mut context,
                        plugin.name(),
                        stage_index + 1,
                        total_stages,
                        output.summary.clone(),
                        Some(output),
                        completed_at,
                        None,
                    )?;
                }
                Err(error) => {
                    let message = format!("plugin `{}` failed: {error}", plugin.name());
                    let failed_at = next_event_time(started_at, &context);
                    self.fail_stage(
                        &mut run,
                        &mut context,
                        stage_index,
                        plugin.name(),
                        total_stages,
                        message.clone(),
                        failed_at,
                    )?;
                    return Err(EduMindError::Research(message));
                }
            }
        }

        let completed_at = next_event_time(started_at, &context);
        run.status = PipelineRunStatus::Completed;
        run.updated_at = completed_at;
        self.record_progress(
            &run,
            &mut context,
            "pipeline",
            total_stages,
            total_stages,
            format!("Completed {} research stages.", total_stages),
            None,
            completed_at,
            None,
        )?;
        let project = self.project_store.record_run(&run, &context)?;
        Ok(ResearchPipelineResult {
            run,
            context,
            project,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn fail_stage(
        &self,
        run: &mut PipelineRun,
        context: &mut PipelineContext,
        stage_index: usize,
        plugin_name: &str,
        total_stages: usize,
        message: String,
        failed_at: DateTime<Utc>,
    ) -> Result<()> {
        run.stages[stage_index].status = PipelineStageStatus::Failed;
        run.stages[stage_index].detail = Some(message.clone());
        run.stages[stage_index].completed_at = Some(failed_at);
        run.status = PipelineRunStatus::Failed;
        run.updated_at = failed_at;
        context.warnings.push(message.clone());
        self.record_progress(
            run,
            context,
            plugin_name,
            stage_index + 1,
            total_stages,
            message.clone(),
            None,
            failed_at,
            Some(&message),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn record_progress(
        &self,
        run: &PipelineRun,
        context: &mut PipelineContext,
        plugin_name: &str,
        completed_stages: usize,
        total_stages: usize,
        message: String,
        output: Option<edumind_core::PluginOutput>,
        at: DateTime<Utc>,
        error: Option<&str>,
    ) -> Result<()> {
        let event = PipelineEvent {
            progress: PipelineProgress {
                run_id: run.id,
                plugin_name: plugin_name.to_owned(),
                completed_stages,
                total_stages,
                message,
                at,
            },
            output,
        };
        context.events.push(event.clone());
        self.run_store.save(run, context, error)?;
        self.run_store.append_event(&event)
    }
}

fn next_event_time(started_at: DateTime<Utc>, context: &PipelineContext) -> DateTime<Utc> {
    let offset = i64::try_from(context.events.len()).unwrap_or(i64::MAX);
    started_at
        .checked_add_signed(Duration::milliseconds(offset))
        .unwrap_or(started_at)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use edumind_core::{LiteratureSource, PaperMetadata, PipelineRunStatus, ResearchRequest};

    use super::ResearchPipelineEngine;
    use crate::{
        memory::MemoryStore,
        research::{
            ConnectorRegistry, ProjectStore, ResearchRunStore, StaticLiteratureConnector,
            TypedResearchPluginRegistry,
        },
    };

    #[tokio::test]
    async fn pipeline_runs_offline_with_static_literature_and_persists_every_stage() {
        let paper = PaperMetadata {
            id: "paper-a".to_owned(),
            title: "Retrieval practice for student learning".to_owned(),
            abstract_text: "This study reports retrieval supports student learning outcomes."
                .to_owned(),
            year: Some(2026),
            citation_count: 12,
            keywords: vec!["retrieval".to_owned(), "education".to_owned()],
            source: LiteratureSource::Manual,
            ..PaperMetadata::default()
        };
        let mut connectors = ConnectorRegistry::empty();
        connectors.register(Arc::new(StaticLiteratureConnector::new(
            LiteratureSource::Manual,
            vec![paper],
        )));
        let run_store = ResearchRunStore::new(MemoryStore::in_memory().unwrap());
        let project_store = ProjectStore::in_memory().unwrap();
        let registry = TypedResearchPluginRegistry::standard(connectors).unwrap();
        let engine = ResearchPipelineEngine::with_registry(run_store, project_store, registry);
        let mut request = ResearchRequest::new("retrieval fairness");
        request.sources = vec![LiteratureSource::Manual];
        request.claims = vec!["Retrieval supports student learning [paper-a]".to_owned()];
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();

        let result = engine.run(request, now).await.unwrap();
        let persisted = engine.get_run(result.run.id).unwrap().unwrap();

        assert_eq!(result.run.status, PipelineRunStatus::Completed);
        assert_eq!(result.context.papers.len(), 1);
        assert!(result.context.graph.is_some());
        assert!(!result.context.insights.is_empty());
        assert!(!result.context.hypotheses.is_empty());
        assert!(result.context.validation.is_some());
        assert_eq!(result.project.papers.len(), 1);
        assert_eq!(result.project.last_run_id, Some(result.run.id));
        assert_eq!(persisted.run.status, PipelineRunStatus::Completed);
        assert!(engine.run_store().events(result.run.id).unwrap().len() >= 15);
    }
}
