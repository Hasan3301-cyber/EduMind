//! Safe runtime helpers for external tools and managed child processes.

pub mod artifacts;
pub mod converter;
pub mod images;
pub mod interpreter;
pub mod latex_compile;
pub mod pdf;
pub mod process_guard;
pub mod service;

pub use artifacts::{
    DocumentArtifact, DocumentConversion, DocumentSummary, RuntimeArtifactStore, Slide, SlideCheck,
    SlideDeck, SlideDeckSnapshot, SlideDeckSummary, SlideOverflow, SlideRender,
};
pub use converter::{ConverterRequest, ExternalConversion, run_converter};
pub use images::{ImageArtifact, ImageEngine, ImageSearchResult};
pub use interpreter::python_executable;
pub use latex_compile::{LatexCompilation, compile_latex};
pub use pdf::{PdfAnalysis, PdfTextExtraction, analyze_pdf, extract_pdf};
pub use process_guard::{
    CapturedOutput, GuardedCommandResult, GuardedCommandSpec, ProcessSandboxReport,
    run_guarded_command,
};
pub use service::{PresentationBuild, RuntimeToolService, RuntimeToolsStatus};
