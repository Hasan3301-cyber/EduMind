import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";

import { RunTimelinePanel } from "./RunTimelinePanel";

const Graph3D = lazy(() => import("./shared/Graph3D").then((module) => ({ default: module.Graph3D })));

const EMPTY_GRAPH = { nodes: [], edges: [], communities: [] };
const MAX_RESEARCH_SOURCE_CHARS = 2048;

export function ResearchWorkspace({ client }) {
  const [topic, setTopic] = useState("");
  const [projects, setProjects] = useState([]);
  const [selected, setSelected] = useState(null);
  const [documents, setDocuments] = useState([]);
  const [question, setQuestion] = useState("");
  const [scopeDraft, setScopeDraft] = useState("");
  const [noteDraft, setNoteDraft] = useState("");
  const [claimsDraft, setClaimsDraft] = useState("");
  const [validationReport, setValidationReport] = useState(null);
  const [exportFormat, setExportFormat] = useState("bibtex");
  const [bibliographyExport, setBibliographyExport] = useState(null);
  const [source, setSource] = useState("");
  const [sourcePaperId, setSourcePaperId] = useState("");
  const [result, setResult] = useState(null);
  const [deepAnswer, setDeepAnswer] = useState(null);
  const [supervision, setSupervision] = useState(null);
  const [gaps, setGaps] = useState(null);
  const [graph, setGraph] = useState(EMPTY_GRAPH);
  const [runId, setRunId] = useState("");
  const [activeRunId, setActiveRunId] = useState(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const refreshProjects = useCallback(async () => {
    if (!client) {
      setProjects([]);
      return [];
    }
    const response = await client.listProjects();
    const nextProjects = Array.isArray(response) ? response : [];
    setProjects(nextProjects);
    setSelected((current) => nextProjects.find((project) => projectId(project) === projectId(current)) ?? current);
    return nextProjects;
  }, [client]);

  useEffect(() => {
    void refreshProjects().catch((reason) => setError(reason.message ?? "EduMind could not load research projects."));
  }, [refreshProjects]);

  const selectedPapers = Array.isArray(selected?.papers) ? selected.papers : [];
  const documentsByPaperId = useMemo(
    () => new Map(documents.map((document) => [documentPaperId(document), document]).filter(([paperId]) => Boolean(paperId))),
    [documents]
  );

  function replaceSelectedProject(project) {
    setSelected(project);
    setProjects((current) => current.map((entry) => projectId(entry) === projectId(project) ? project : entry));
  }

  async function run(label, action, { showResult = true } = {}) {
    if (!client) {
      setError("Launch the desktop app to run the local research workspace.");
      return null;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await action();
      if (showResult) {
        setResult({ label, response });
      }
      const responseRunId = response?.run_id ?? response?.runId;
      if (responseRunId) {
        setActiveRunId(String(responseRunId));
      }
      return response;
    } catch (reason) {
      setError(reason.message ?? "EduMind could not complete that research action.");
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function startResearch(event) {
    event.preventDefault();
    if (!topic.trim()) {
      setStatus("Add a focused research topic before starting discovery.");
      return;
    }
    const response = await run("Focused research run", () => client.runFocusedResearch({
      topic: topic.trim(),
      query: topic.trim(),
      maxResults: 12
    }));
    if (response) {
      try {
        await refreshProjects();
        setStatus("Discovery completed. Select the project to choose open-access papers for full-text analysis.");
      } catch (reason) {
        setError(reason.message ?? "Research finished, but EduMind could not refresh the project list.");
      }
    }
  }

  async function selectProject(project) {
    const id = projectId(project);
    if (!id) {
      return;
    }
    const loaded = await run("Research project", async () => {
      const [projectResponse, documentResponse] = await Promise.all([
        client.researchProject(id),
        client.researchDocuments(id)
      ]);
      return {
        project: projectResponse,
        documents: Array.isArray(documentResponse) ? documentResponse : []
      };
    }, { showResult: false });
    if (loaded?.project) {
      setSelected(loaded.project);
      setScopeDraft(cleanText(loaded.project.scope, 2000));
      setNoteDraft("");
      setClaimsDraft("");
      setValidationReport(null);
      setBibliographyExport(null);
      setDocuments(loaded.documents);
      setSourcePaperId("");
      setDeepAnswer(null);
      setSupervision(null);
      setGaps(null);
      setGraph(EMPTY_GRAPH);
      setResult(null);
      setStatus(`${loaded.documents.length} indexed ${loaded.documents.length === 1 ? "paper body is" : "paper bodies are"} available for this project.`);
    }
  }

  async function refreshDocuments() {
    if (!selected) {
      return;
    }
    const response = await run("Indexed paper bodies", () => client.researchDocuments(projectId(selected)), { showResult: false });
    if (Array.isArray(response)) {
      setDocuments(response);
      setStatus(`${response.length} indexed ${response.length === 1 ? "paper body is" : "paper bodies are"} available for full-text analysis.`);
    }
  }

  async function askProject(deep = false) {
    if (!selected || !question.trim()) {
      setStatus("Ask a specific question about the selected research project.");
      return;
    }
    const id = projectId(selected);
    const response = await run(
      deep ? "Full-text answer" : "Project answer",
      () => deep
        ? client.deepResearchAnswer(id, question.trim(), { limit: 5 })
        : client.researchProjectAnswer(id, question.trim(), { limit: 5 }),
      { showResult: !deep }
    );
    if (deep && response) {
      setDeepAnswer(response);
      setStatus("Full-text answer is grounded in the indexed paper passages shown below.");
    }
  }

  async function saveProjectScope() {
    if (!selected) {
      return;
    }
    const scope = scopeDraft.trim();
    if (!scope) {
      setStatus("Define the project scope before saving or exporting research.");
      return;
    }
    if (!window.confirm("Save this scope as the canonical boundary for the selected research project?")) {
      return;
    }
    const updated = await run(
      "Project scope",
      () => client.setResearchScope(projectId(selected), scope),
      { showResult: false }
    );
    if (updated) {
      replaceSelectedProject(updated);
      setScopeDraft(cleanText(updated.scope, 2000));
      setBibliographyExport(null);
      setStatus("Saved the project scope. Future synthesis and export decisions can use this boundary.");
    }
  }

  async function saveProjectQuestion() {
    if (!selected || !question.trim()) {
      setStatus("Write a research question before saving it to the project.");
      return;
    }
    if (!window.confirm("Save this question to the selected research project?")) {
      return;
    }
    const updated = await run(
      "Saved research question",
      () => client.addResearchQuestion(projectId(selected), question.trim()),
      { showResult: false }
    );
    if (updated) {
      replaceSelectedProject(updated);
      setStatus("Saved the question to the canonical research project.");
    }
  }

  async function saveProjectNote() {
    if (!selected || !noteDraft.trim()) {
      setStatus("Write a source-aware note before saving it to the project.");
      return;
    }
    if (!window.confirm("Save this note to the selected local research project?")) {
      return;
    }
    const updated = await run(
      "Saved project note",
      () => client.addResearchNote(projectId(selected), noteDraft.trim()),
      { showResult: false }
    );
    if (updated) {
      replaceSelectedProject(updated);
      setNoteDraft("");
      setStatus("Saved the note to the canonical research project.");
    }
  }

  async function validateDraftClaims() {
    if (!selected) {
      return;
    }
    const claims = parseDraftClaims(claimsDraft);
    if (!claims.length) {
      setStatus("Add one draft claim per line before validating.");
      return;
    }
    const evidence = buildClaimEvidence(selectedPapers, deepAnswer);
    if (!evidence.length) {
      setStatus("No abstract or cited passage evidence is available for claim validation.");
      return;
    }
    const report = await run(
      "Claim validation",
      () => client.validateResearchClaims({ claims, evidence, supportThreshold: 0.18 }),
      { showResult: false }
    );
    if (report) {
      setValidationReport(report);
      setStatus("Validated draft claims against the project evidence shown in this workspace. Unsupported claims remain clearly labeled.");
    }
  }

  async function exportBibliography() {
    if (!selected) {
      return;
    }
    if (!String(selected.scope ?? "").trim()) {
      setStatus("Save an explicit project scope before exporting a bibliography.");
      return;
    }
    const formatLabel = exportFormat === "ris" ? "RIS" : "BibTeX";
    if (!window.confirm(`Generate a ${formatLabel} bibliography for the selected scoped project?`)) {
      return;
    }
    const exported = await run(
      `${formatLabel} bibliography`,
      () => client.exportResearchProject(projectId(selected), exportFormat),
      { showResult: false }
    );
    if (exported) {
      setBibliographyExport(exported);
      setStatus(`Generated a ${formatLabel} bibliography from ${selectedPapers.length} project ${selectedPapers.length === 1 ? "paper" : "papers"}.`);
    }
  }

  async function copyBibliography() {
    const content = bibliographyExport?.content;
    if (!content || !globalThis.navigator?.clipboard) {
      setStatus("Clipboard access is unavailable. Select the bibliography text and copy it manually.");
      return;
    }
    try {
      await globalThis.navigator.clipboard.writeText(content);
      setStatus("Copied the generated bibliography to the clipboard.");
    } catch (reason) {
      setError(reason.message ?? "EduMind could not copy the bibliography.");
    }
  }

  async function downloadAndIndexPaper(paper) {
    if (!selected) {
      return;
    }
    const url = downloadablePaperUrl(paper);
    if (!url) {
      setError("This paper has no known HTTPS open-access PDF. Paste a permitted PDF source after choosing the paper instead.");
      return;
    }
    const paperTitle = cleanText(paper?.title, 160) || "this paper";
    if (typeof window !== "undefined" && !window.confirm(`Download and index “${paperTitle}” from its known open-access HTTPS source? EduMind will save only the extracted local research index.`)) {
      return;
    }
    await ingestPdf(`Download and index: ${paperTitle}`, {
      paperId: paperId(paper),
      ocr: "auto"
    });
  }

  async function ingestManualSource() {
    if (!selected) {
      return;
    }
    const sourceResult = validatePdfSource(source);
    if (sourceResult.error) {
      setError(sourceResult.error);
      return;
    }
    const fallbackPaperId = selectedPapers.length === 1 ? paperId(selectedPapers[0]) : "";
    const paperIdValue = sourcePaperId || fallbackPaperId;
    const paper = selectedPapers.find((candidate) => paperId(candidate) === paperIdValue);
    if (!paperIdValue || !paper) {
      setError("Choose the project paper that this PDF belongs to before indexing it.");
      return;
    }
    const paperTitle = cleanText(paper.title, 160) || "the selected paper";
    if (typeof window !== "undefined" && !window.confirm(`Index this PDF as “${paperTitle}”? EduMind will extract searchable text locally and will not replace another planner or research record.`)) {
      return;
    }
    await ingestPdf(`Index supplied PDF: ${paperTitle}`, {
      paperId: paperIdValue,
      source: sourceResult.source,
      title: paperTitle,
      ocr: "auto"
    });
  }

  async function ingestPdf(label, request) {
    if (!selected) {
      return;
    }
    const document = await run(label, () => client.ingestResearchPdf(projectId(selected), request), { showResult: false });
    if (document) {
      setDocuments((current) => upsertDocument(current, document));
      setDeepAnswer(null);
      setSupervision(null);
      setGaps(null);
      setStatus(`Indexed “${cleanText(document.title, 160) || "research paper"}” into ${Number(document.chunk_count ?? document.chunkCount ?? 0).toLocaleString()} searchable chunks.`);
    }
  }

  async function reviewGaps() {
    if (!selected) {
      return;
    }
    const response = await run("Research gaps", () => client.researchGaps(projectId(selected)), { showResult: false });
    if (response) {
      setGaps(response);
      setStatus("Displayed only author-stated limitations, future work, and open questions found in indexed paper bodies.");
    }
  }

  async function superviseProject() {
    if (!selected) {
      return;
    }
    const response = await run("Supervision plan", () => client.researchSupervision(projectId(selected)), { showResult: false });
    if (response) {
      setSupervision(response);
      setStatus("Supervisor guidance is based on the saved project and indexed full-text corpus.");
    }
  }

  async function synthesizeProject() {
    if (!selected) {
      return;
    }
    await run("Project synthesis", () => client.researchSynthesis(projectId(selected)));
  }

  async function literatureGraph() {
    if (!selected) {
      return;
    }
    const response = await run("Literature graph", () => client.researchLiteratureGraph(selectedPapers));
    if (response) {
      setGraph(response);
    }
  }

  return (
    <section className="research-workspace premium-research-workspace">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Research Assistant + Supervisor</p>
          <h1>Discover papers, inspect their evidence, and get a defensible next step.</h1>
          <p className="research-workspace-intro">EduMind downloads only a paper you choose, indexes its full text locally, and keeps advice linked to evidence and clear gaps.</p>
        </div>
        <button type="button" className="secondary-button" onClick={() => void refreshProjects().catch((reason) => setError(reason.message ?? "EduMind could not refresh research projects."))} disabled={busy}>
          <i className="fa-solid fa-rotate" aria-hidden="true" /> Refresh projects
        </button>
      </div>
      <form className="research-run-form" onSubmit={startResearch}>
        <label htmlFor="research-topic">
          <span>Research topic</span>
          <input id="research-topic" value={topic} onChange={(event) => setTopic(event.target.value)} placeholder="How does retrieval practice improve retention?" maxLength="280" />
        </label>
        <button type="submit" disabled={busy}>
          <i className="fa-solid fa-magnifying-glass" aria-hidden="true" /> Run focused research
        </button>
      </form>
      <div className="premium-run-inspector">
        <label htmlFor="research-run-id">
          <span>Premium run ID</span>
          <input id="research-run-id" value={runId} onChange={(event) => setRunId(event.target.value)} placeholder="Paste a run ID to inspect evidence" />
        </label>
        <button type="button" className="secondary-button" onClick={() => setActiveRunId(runId.trim() || null)} disabled={!runId.trim() || busy}>
          Inspect run timeline
        </button>
      </div>
      {status && <p className="research-status" role="status">{status}</p>}
      {error && <p className="error-message" role="alert">{error}</p>}
      <div className="research-layout">
        <aside className="project-list" aria-label="Research projects">
          <h2>Projects</h2>
          {projects.length ? projects.map((project) => (
            <button
              type="button"
              key={projectId(project)}
              className={projectId(project) === projectId(selected) ? "project-button active" : "project-button"}
              onClick={() => void selectProject(project)}
              disabled={busy}
            >
              <strong>{cleanText(project.topic, 96) || "Untitled project"}</strong>
              <small>{Array.isArray(project.papers) ? project.papers.length : 0} papers</small>
            </button>
          )) : <p className="muted">Run a topic to create the first project.</p>}
        </aside>
        <div className="research-detail">
          {selected ? (
            <>
              <div className="selected-project-heading">
                <div>
                  <p className="eyebrow">Selected project</p>
                  <h2>{cleanText(selected.topic, 180) || "Research project"}</h2>
                </div>
                <div className="research-project-counts" aria-label="Project evidence summary">
                  <span><i className="fa-solid fa-book-open" aria-hidden="true" /> {selectedPapers.length} papers</span>
                  <span><i className="fa-solid fa-file-lines" aria-hidden="true" /> {documents.length} indexed PDFs</span>
                </div>
              </div>

              <section className="research-project-governance" aria-label="Research project scope and notes">
                <article>
                  <div className="research-command-heading">
                    <i className="fa-solid fa-bullseye" aria-hidden="true" />
                    <div>
                      <h3>Project scope</h3>
                      <p>Define what this project will and will not claim before synthesis or export.</p>
                    </div>
                  </div>
                  <label htmlFor="research-project-scope">
                    <span>Canonical scope</span>
                    <textarea id="research-project-scope" value={scopeDraft} onChange={(event) => setScopeDraft(event.target.value)} maxLength="2000" rows="4" placeholder="Population, intervention, outcomes, date range, and exclusions" disabled={busy} />
                  </label>
                  <button type="button" onClick={() => void saveProjectScope()} disabled={busy || !scopeDraft.trim()}>Save project scope</button>
                </article>
                <article>
                  <div className="research-command-heading">
                    <i className="fa-solid fa-note-sticky" aria-hidden="true" />
                    <div>
                      <h3>Source-aware notes</h3>
                      <p>Save your interpretation separately from paper evidence and generated synthesis.</p>
                    </div>
                  </div>
                  <label htmlFor="research-project-note">
                    <span>New project note</span>
                    <textarea id="research-project-note" value={noteDraft} onChange={(event) => setNoteDraft(event.target.value)} maxLength="3000" rows="4" placeholder="My interpretation, uncertainty, or next comparison…" disabled={busy} />
                  </label>
                  <button type="button" onClick={() => void saveProjectNote()} disabled={busy || !noteDraft.trim()}>Save project note</button>
                  {Array.isArray(selected.notes) && selected.notes.length > 0 && (
                    <ul className="research-saved-items" aria-label="Saved project notes">
                      {selected.notes.slice(-3).reverse().map((note) => <li key={note.id}><span>Student note</span>{cleanText(note.content, 320)}</li>)}
                    </ul>
                  )}
                </article>
              </section>

              <div className="research-command-grid">
                <section className="research-command-card research-intake-card">
                  <div className="research-command-heading">
                    <i className="fa-solid fa-file-arrow-down" aria-hidden="true" />
                    <div>
                      <h3>Index a PDF you choose</h3>
                      <p>Use a local path or HTTPS source. Large files, non-PDF bytes, and insecure URLs are rejected by the gateway.</p>
                    </div>
                  </div>
                  <label htmlFor="research-source-paper">
                    <span>Attach source to paper</span>
                    <select id="research-source-paper" value={sourcePaperId} onChange={(event) => setSourcePaperId(event.target.value)} disabled={busy}>
                      <option value="">{selectedPapers.length === 1 ? "Use the only project paper" : "Choose a project paper"}</option>
                      {selectedPapers.map((paper) => <option value={paperId(paper)} key={paperId(paper)}>{cleanText(paper.title, 110) || paperId(paper)}</option>)}
                    </select>
                  </label>
                  <label htmlFor="research-pdf-source">
                    <span>PDF URL or local path</span>
                    <input id="research-pdf-source" value={source} onChange={(event) => setSource(event.target.value)} placeholder="https://example.org/open-paper.pdf or D:\\papers\\study.pdf" maxLength={MAX_RESEARCH_SOURCE_CHARS} disabled={busy} />
                  </label>
                  <div className="research-inline-actions">
                    <button type="button" onClick={() => void ingestManualSource()} disabled={busy || !source.trim()}>
                      <i className="fa-solid fa-file-circle-plus" aria-hidden="true" /> Index supplied PDF
                    </button>
                    <button type="button" className="secondary-button" onClick={() => void refreshDocuments()} disabled={busy}>
                      Refresh index
                    </button>
                  </div>
                </section>

                <section className="research-command-card research-question-card">
                  <div className="research-command-heading">
                    <i className="fa-solid fa-microscope" aria-hidden="true" />
                    <div>
                      <h3>Ask the evidence</h3>
                      <p>Use full-text analysis for answers with cited passages, not a generic summary.</p>
                    </div>
                  </div>
                  <label htmlFor="research-question">
                    <span>Research question</span>
                    <input id="research-question" value={question} onChange={(event) => setQuestion(event.target.value)} placeholder="What are the strongest limitations across these studies?" maxLength="500" disabled={busy} />
                  </label>
                  <div className="research-inline-actions">
                    <button type="button" className="secondary-button" onClick={() => void askProject(false)} disabled={busy || !question.trim()}>Ask abstracts</button>
                    <button type="button" onClick={() => void askProject(true)} disabled={busy || !question.trim()}>
                      <i className="fa-solid fa-quote-left" aria-hidden="true" /> Analyze indexed papers
                    </button>
                    <button type="button" className="secondary-button" onClick={() => void saveProjectQuestion()} disabled={busy || !question.trim()}>
                      <i className="fa-solid fa-bookmark" aria-hidden="true" /> Save question
                    </button>
                  </div>
                  {Array.isArray(selected.questions) && selected.questions.length > 0 && (
                    <ul className="research-saved-items" aria-label="Saved research questions">
                      {selected.questions.slice(-4).reverse().map((savedQuestion, index) => <li key={`${savedQuestion}-${index}`}><span>Saved question</span>{cleanText(savedQuestion, 320)}</li>)}
                    </ul>
                  )}
                </section>

                <section className="research-command-card research-supervisor-card">
                  <div className="research-command-heading">
                    <i className="fa-solid fa-compass-drafting" aria-hidden="true" />
                    <div>
                      <h3>Research supervision</h3>
                      <p>Check corpus coverage, author-stated gaps, and the most useful next reading actions.</p>
                    </div>
                  </div>
                  <div className="research-supervisor-actions">
                    <button type="button" className="secondary-button" onClick={() => void reviewGaps()} disabled={busy}>Review gaps</button>
                    <button type="button" onClick={() => void superviseProject()} disabled={busy}>Supervise project</button>
                    <button type="button" className="secondary-button" onClick={() => void synthesizeProject()} disabled={busy}>Synthesis</button>
                    <button type="button" className="secondary-button" onClick={() => void literatureGraph()} disabled={busy}>Literature graph</button>
                  </div>
                </section>
              </div>

              <section className="research-validation-export" aria-label="Claim validation and bibliography export">
                <article>
                  <div className="research-command-heading">
                    <i className="fa-solid fa-scale-balanced" aria-hidden="true" />
                    <div>
                      <h3>Validate draft claims</h3>
                      <p>Enter one claim per line. EduMind compares each claim with visible abstracts and cited full-text passages.</p>
                    </div>
                  </div>
                  <label htmlFor="research-draft-claims">
                    <span>Draft claims</span>
                    <textarea id="research-draft-claims" value={claimsDraft} onChange={(event) => setClaimsDraft(event.target.value)} rows="5" maxLength="6000" placeholder={"Retrieval practice improves delayed retention.\nThe effect is identical in every subject."} disabled={busy} />
                  </label>
                  <button type="button" onClick={() => void validateDraftClaims()} disabled={busy || !claimsDraft.trim()}>Validate against project evidence</button>
                  {validationReport && <ClaimValidationPanel report={validationReport} />}
                </article>
                <article>
                  <div className="research-command-heading">
                    <i className="fa-solid fa-file-export" aria-hidden="true" />
                    <div>
                      <h3>Export scoped bibliography</h3>
                      <p>Choose a format after saving scope. Generated text stays local until you copy or save it.</p>
                    </div>
                  </div>
                  <label htmlFor="research-export-format">
                    <span>Bibliography format</span>
                    <select id="research-export-format" value={exportFormat} onChange={(event) => { setExportFormat(event.target.value); setBibliographyExport(null); }} disabled={busy}>
                      <option value="bibtex">BibTeX</option>
                      <option value="ris">RIS</option>
                    </select>
                  </label>
                  <button type="button" onClick={() => void exportBibliography()} disabled={busy || !String(selected.scope ?? "").trim()}>Generate bibliography</button>
                  {!String(selected.scope ?? "").trim() && <p className="muted">Save the project scope to enable export.</p>}
                  {bibliographyExport && (
                    <div className="research-bibliography-result">
                      <div><strong>{String(bibliographyExport.format).toUpperCase()}</strong><button type="button" className="text-button" onClick={() => void copyBibliography()}>Copy</button></div>
                      <pre>{bibliographyExport.content}</pre>
                    </div>
                  )}
                </article>
              </section>

              <section className="research-document-library" aria-labelledby="indexed-papers-title">
                <div className="research-section-heading">
                  <div>
                    <p className="eyebrow">Local evidence</p>
                    <h3 id="indexed-papers-title">Indexed paper bodies</h3>
                  </div>
                  <span>{documents.length} ready for deep analysis</span>
                </div>
                {documents.length ? (
                  <ul>
                    {documents.map((document) => (
                      <li key={`${documentPaperId(document)}-${document.extracted_at ?? document.extractedAt ?? "document"}`}>
                        <i className="fa-solid fa-file-pdf" aria-hidden="true" />
                        <div>
                          <strong>{cleanText(document.title, 160) || "Indexed research paper"}</strong>
                          <span>{Number(document.chunk_count ?? document.chunkCount ?? 0).toLocaleString()} chunks · {Number(document.char_count ?? document.charCount ?? 0).toLocaleString()} characters{document.ocr ? " · OCR used" : ""}</span>
                        </div>
                      </li>
                    ))}
                  </ul>
                ) : <p className="muted">No PDF body is indexed yet. Choose an open-access paper below or supply a PDF source you have permission to use.</p>}
              </section>

              <section className="research-paper-library" aria-labelledby="project-papers-title">
                <div className="research-section-heading">
                  <div>
                    <p className="eyebrow">Paper library</p>
                    <h3 id="project-papers-title">Choose a paper to download and analyze</h3>
                  </div>
                  <span>Downloads require your confirmation</span>
                </div>
                {selectedPapers.length ? (
                  <div className="research-paper-grid">
                    {selectedPapers.slice(0, 12).map((paper) => {
                      const paperIdentifier = paperId(paper);
                      const indexedDocument = documentsByPaperId.get(paperIdentifier);
                      const openAccessUrl = downloadablePaperUrl(paper);
                      return (
                        <article className="research-paper-card" key={paperIdentifier || cleanText(paper.title, 180)}>
                          <div className="research-paper-card-heading">
                            <span className={indexedDocument ? "research-paper-state indexed" : openAccessUrl ? "research-paper-state available" : "research-paper-state unavailable"}>
                              {indexedDocument ? "Indexed" : openAccessUrl ? "Open access found" : "PDF unavailable"}
                            </span>
                            <span>{formatPaperMeta(paper)}</span>
                          </div>
                          <h4>{cleanText(paper.title, 180) || "Untitled paper"}</h4>
                          {paper.abstract_text && <p>{cleanText(paper.abstract_text, 260)}</p>}
                          <div className="research-paper-card-footer">
                            <span>{indexedDocument ? `${Number(indexedDocument.chunk_count ?? indexedDocument.chunkCount ?? 0).toLocaleString()} indexed chunks` : openAccessUrl ? "Known HTTPS source" : "Add a permitted PDF source manually"}</span>
                            <button type="button" onClick={() => void downloadAndIndexPaper(paper)} disabled={busy || !openAccessUrl}>
                              <i className={`fa-solid ${indexedDocument ? "fa-arrows-rotate" : "fa-download"}`} aria-hidden="true" />
                              {indexedDocument ? "Re-index PDF" : "Download & index PDF"}
                            </button>
                          </div>
                        </article>
                      );
                    })}
                  </div>
                ) : <p className="muted">This project does not yet contain papers from discovery.</p>}
              </section>

              {deepAnswer && <DeepEvidencePanel answer={deepAnswer} />}
              {supervision && <SupervisionPanel supervision={supervision} />}
              {gaps && <GapPanel report={gaps} />}
            </>
          ) : (
            <div className="empty-state">Select a project to expose the complete assistant and supervisor loop.</div>
          )}
          {result && (
            <section className="research-result" aria-live="polite">
              <h3>{result.label}</h3>
              <pre>{JSON.stringify(result.response, null, 2)}</pre>
            </section>
          )}
        </div>
      </div>
      {selected && (
        <Suspense fallback={<p className="muted">Loading the local 3D renderer…</p>}>
          <Graph3D graph={graph} title="Research literature graph" />
        </Suspense>
      )}
      <RunTimelinePanel client={client} runId={activeRunId} />
    </section>
  );
}

function DeepEvidencePanel({ answer }) {
  const passages = Array.isArray(answer?.passages) ? answer.passages : [];
  const warnings = Array.isArray(answer?.warnings) ? answer.warnings : [];
  const groundedAnswer = answer?.grounded_answer ?? answer?.groundedAnswer;
  const citations = Array.isArray(groundedAnswer?.citations) ? groundedAnswer.citations : [];
  const citedPassageCount = citations.length || passages.length;
  return (
    <section className="research-evidence-panel" aria-labelledby="fulltext-answer-title">
      <div className="research-section-heading">
        <div>
          <p className="eyebrow">Full-text answer</p>
          <h3 id="fulltext-answer-title">Evidence from indexed papers</h3>
        </div>
        <span>{citedPassageCount} cited {citedPassageCount === 1 ? "passage" : "passages"}</span>
      </div>
      <p className="research-evidence-answer">{cleanText(answer?.answer, 3000) || "No grounded full-text answer was returned."}</p>
      {warnings.length > 0 && <ul className="research-warning-list">{warnings.map((warning, index) => <li key={`${warning}-${index}`}>{cleanText(warning, 260)}</li>)}</ul>}
      {passages.length > 0 && (
        <ol className="research-passage-list">
          {passages.map((passage) => (
            <li key={`${passage.paper_id ?? passage.paperId}-${passage.chunk_index ?? passage.chunkIndex}`}>
              <div>
                <strong>{cleanText(passage.title, 160) || "Indexed paper"}</strong>
                <span>Chunk {Number(passage.chunk_index ?? passage.chunkIndex ?? 0) + 1} · relevance {formatScore(passage.score)}</span>
              </div>
              <blockquote>{cleanText(passage.text, 520)}</blockquote>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

function SupervisionPanel({ supervision }) {
  const health = supervision?.corpus_health ?? supervision?.corpusHealth ?? {};
  const nextSteps = Array.isArray(supervision?.next_steps ?? supervision?.nextSteps) ? supervision.next_steps ?? supervision.nextSteps : [];
  const readingPlan = Array.isArray(supervision?.reading_plan ?? supervision?.readingPlan) ? supervision.reading_plan ?? supervision.readingPlan : [];
  const openQuestions = Array.isArray(supervision?.open_questions ?? supervision?.openQuestions) ? supervision.open_questions ?? supervision.openQuestions : [];
  return (
    <section className="research-supervision-panel" aria-labelledby="supervision-title">
      <div className="research-section-heading">
        <div>
          <p className="eyebrow">Supervisor guidance</p>
          <h3 id="supervision-title">What to read and do next</h3>
        </div>
        <span>{Number(health.ingested_documents ?? health.ingestedDocuments ?? 0)} indexed sources</span>
      </div>
      <div className="research-health-grid">
        <article><span>Total papers</span><strong>{Number(health.total_papers ?? health.totalPapers ?? 0)}</strong></article>
        <article><span>Indexed PDFs</span><strong>{Number(health.ingested_documents ?? health.ingestedDocuments ?? 0)}</strong></article>
        <article><span>Newest year</span><strong>{health.newest_paper_year ?? health.newestPaperYear ?? "Unknown"}</strong></article>
      </div>
      {nextSteps.length > 0 && <ResearchList title="Next steps" items={nextSteps} />}
      {openQuestions.length > 0 && <ResearchList title="Open questions" items={openQuestions} />}
      {readingPlan.length > 0 && (
        <ol className="research-reading-plan">
          {readingPlan.map((item) => (
            <li key={`${item.paper_id ?? item.paperId}-${item.priority}`}>
              <span>{item.priority}</span>
              <div><strong>{cleanText(item.title, 160) || "Research paper"}</strong><p>{cleanText(item.rationale, 260)}</p></div>
              <small>{item.ingested ? "Indexed" : "Not indexed"}</small>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

function GapPanel({ report }) {
  const health = report?.corpus_health ?? report?.corpusHealth ?? {};
  const statedGaps = Array.isArray(report?.stated_gaps ?? report?.statedGaps) ? report.stated_gaps ?? report.statedGaps : [];
  return (
    <section className="research-gap-panel" aria-labelledby="research-gaps-title">
      <div className="research-section-heading">
        <div>
          <p className="eyebrow">Author-stated evidence gaps</p>
          <h3 id="research-gaps-title">Limitations and future work</h3>
        </div>
        <span>{Number(health.ingested_documents ?? health.ingestedDocuments ?? 0)} indexed sources searched</span>
      </div>
      {statedGaps.length ? (
        <ul className="research-gap-list">
          {statedGaps.map((gap, index) => (
            <li key={`${gap.provenance?.paper_id ?? gap.provenance?.paperId}-${index}`}>
              <span>{cleanText(gap.kind, 40).replace(/_/g, " ")}</span>
              <blockquote>{cleanText(gap.excerpt, 520)}</blockquote>
              <small>{cleanText(gap.provenance?.title, 160) || "Indexed paper"} · {cleanText(gap.provenance?.locator, 80)}</small>
            </li>
          ))}
        </ul>
      ) : <p className="muted">No author-stated gaps were found in the current indexed papers. This is not evidence that the topic has no gaps.</p>}
    </section>
  );
}

function ClaimValidationPanel({ report }) {
  const claims = Array.isArray(report?.claims) ? report.claims : [];
  const issues = [
    ...(Array.isArray(report?.hallucinations) ? report.hallucinations : []),
    ...(Array.isArray(report?.citation_errors) ? report.citation_errors : []),
    ...(Array.isArray(report?.logical_issues) ? report.logical_issues : []),
    ...(Array.isArray(report?.bias_flags) ? report.bias_flags : [])
  ];
  return (
    <section className="claim-validation-report" aria-label="Claim validation report">
      <div><strong>Evidence score</strong><span>{formatScore(report?.overall_score)}</span></div>
      <ul>
        {claims.map((claim, index) => (
          <li key={`${claim.claim}-${index}`}>
            <span className={`claim-support support-${claim.support}`}>{cleanText(claim.support, 32).replace(/_/g, " ")}</span>
            <strong>{cleanText(claim.claim, 600)}</strong>
            <small>{formatScore(claim.support_score)} support · {(claim.evidence_ids ?? []).length} evidence links</small>
          </li>
        ))}
      </ul>
      {issues.length > 0 && (
        <ul className="research-warning-list">
          {issues.map((issue, index) => <li key={`${issue.code}-${index}`}>{cleanText(issue.message, 320)}</li>)}
        </ul>
      )}
    </section>
  );
}

function ResearchList({ title, items }) {
  return (
    <section className="research-guidance-list">
      <h4>{title}</h4>
      <ul>{items.map((item, index) => <li key={`${item}-${index}`}>{cleanText(item, 320)}</li>)}</ul>
    </section>
  );
}

function parseDraftClaims(value) {
  const seen = new Set();
  return String(value ?? "")
    .split(/\r?\n/)
    .map((claim) => cleanText(claim, 800))
    .filter((claim) => {
      const key = claim.toLowerCase();
      if (!claim || seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    })
    .slice(0, 12);
}

function buildClaimEvidence(papers, deepAnswer) {
  const evidence = [];
  for (const paper of papers.slice(0, 30)) {
    const id = paperId(paper);
    const text = cleanText(paper?.abstract_text ?? paper?.abstractText, 5000);
    if (!id || !text) {
      continue;
    }
    evidence.push({
      id,
      title: cleanText(paper.title, 240) || "Project paper",
      text,
      citation_label: id,
      source_url: downloadablePaperUrl(paper) || undefined
    });
  }
  const passages = Array.isArray(deepAnswer?.passages) ? deepAnswer.passages : [];
  for (const passage of passages.slice(0, 20)) {
    const sourcePaperId = cleanText(passage.paper_id ?? passage.paperId, 180);
    const chunkIndex = Number(passage.chunk_index ?? passage.chunkIndex ?? 0);
    const text = cleanText(passage.text, 5000);
    if (!sourcePaperId || !text) {
      continue;
    }
    evidence.push({
      id: `${sourcePaperId}#${chunkIndex}`,
      title: cleanText(passage.title, 240) || "Indexed paper passage",
      text,
      citation_label: sourcePaperId
    });
  }
  return evidence;
}

function projectId(project) {
  if (!project?.id) {
    return "";
  }
  return typeof project.id === "string" ? project.id : project.id.value ?? String(project.id);
}

function paperId(paper) {
  return cleanText(paper?.id, 180);
}

function documentPaperId(document) {
  return cleanText(document?.paper_id ?? document?.paperId, 180);
}

function downloadablePaperUrl(paper) {
  const candidates = [paper?.open_access_url, paper?.openAccessUrl, paper?.source_url, paper?.sourceUrl];
  return candidates.find((candidate) => isHttpsUrl(candidate)) ?? "";
}

function isHttpsUrl(value) {
  try {
    const url = new URL(String(value ?? ""));
    return url.protocol === "https:" && Boolean(url.hostname);
  } catch {
    return false;
  }
}

function validatePdfSource(value) {
  const source = String(value ?? "").trim();
  if (!source) {
    return { error: "Provide a local PDF path or HTTPS PDF URL." };
  }
  if (source.length > MAX_RESEARCH_SOURCE_CHARS) {
    return { error: "Keep the PDF source below 2,048 characters." };
  }
  if (/^https?:\/\//i.test(source) && !isHttpsUrl(source)) {
    return { error: "Remote research PDFs must use a valid HTTPS URL." };
  }
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(source)) {
    return { error: "Use a local file path or HTTPS PDF URL." };
  }
  return { source };
}

function upsertDocument(currentDocuments, document) {
  const documentId = documentPaperId(document);
  const withoutExisting = currentDocuments.filter((current) => documentPaperId(current) !== documentId);
  return [...withoutExisting, document].sort((left, right) => cleanText(left.title, 180).localeCompare(cleanText(right.title, 180)));
}

function formatPaperMeta(paper) {
  const year = paper?.year ?? "n.d.";
  const source = cleanText(paper?.source, 48) || "research source";
  const citations = Number(paper?.citation_count ?? paper?.citationCount ?? 0);
  return `${year} · ${source}${citations ? ` · ${citations.toLocaleString()} citations` : ""}`;
}

function formatScore(value) {
  const score = Number(value);
  return Number.isFinite(score) ? `${Math.round(score * 100)}%` : "ranked";
}

function cleanText(value, limit) {
  if (typeof value !== "string") {
    return "";
  }
  return Array.from(value)
    .filter((character) => {
      const code = character.codePointAt(0);
      return code !== undefined && code >= 32 && code !== 127;
    })
    .join("")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, limit);
}
