//! Library Registry for managing loaded Scheme libraries
//!
//! The LibraryRegistry maintains a global registry of loaded libraries and manages:
//! - Library name → Library mapping
//! - Search paths for finding library files
//! - Circular dependency detection
//! - Library loading and caching

use crate::library::Library;
use patina_core::{Environment, FileSystem, TaggedValue};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// A library's place on the loading stack, taken by
/// [`LibraryRegistry::begin_loading_scoped`] and given back when dropped.
#[must_use = "dropping it ends the load at once"]
pub struct Loading {
    registry: Rc<RefCell<LibraryRegistry>>,
    name: Vec<String>,
}

impl Loading {
    /// Root `env`, the environment the library is built in, and `body`, its
    /// forms not yet run, in this load's entry until the load ends: what
    /// [`ParsedLibrary::hand_to`](crate::library_loader::ParsedLibrary::hand_to)
    /// does, so that the load may collect (#677).
    pub(crate) fn hold(&self, env: Rc<Environment>, body: Vec<TaggedValue>) {
        let mut registry = self.registry.borrow_mut();
        let entry = registry.entry_mut(&self.name).expect(
            "a load's entry stays on the loading stack until its `Loading` drops, unless the \
             registry is cleared under it",
        );
        entry.env = Some(env);
        entry.body = body.into();
    }

    /// The next body form, taken out of this load's entry, which roots it no
    /// longer; `None` once every form has been taken. The loader holds the
    /// form only while it expands it, under `desugar_with_imports`' guard.
    pub fn next_form(&self) -> Option<TaggedValue> {
        self.registry
            .borrow_mut()
            .entry_mut(&self.name)?
            .body
            .pop_front()
    }
}

impl Drop for Loading {
    fn drop(&mut self) {
        // Not while the registry is borrowed, which only an unwinding panic
        // can leave it: a second panic would abort the process.
        if let Ok(mut registry) = self.registry.try_borrow_mut() {
            registry.end_loading(&self.name);
        }
    }
}

/// Error types for library operations
#[derive(Debug, Clone)]
pub enum LibraryError {
    /// Library not found in any search path. `searched` is the path a loader
    /// looked through, in order — what someone has to fix — and is empty
    /// where the failure was a lookup among libraries already loaded
    /// ([`LibraryError::not_found`]).
    NotFound {
        name: Vec<String>,
        searched: Vec<PathBuf>,
    },

    /// Circular dependency detected during loading
    CircularDependency(Vec<Vec<String>>),

    /// Library with this name is already loaded (should not normally occur)
    AlreadyLoaded(Vec<String>),

    /// Invalid library name (empty or malformed)
    InvalidName(String),

    /// I/O error while reading library file
    IoError(String),

    /// Parse error in library file
    ParseError { file: String, message: String },

    /// A post-reader failure with its original machine-readable category.
    ProcessingError {
        file: String,
        message: String,
        diagnostic: Box<crate::Diagnostic>,
        location: Option<patina_core::SourceLocation>,
    },

    /// Evaluation failed after parsing. Keep the error typed: a continuation
    /// escaping a library load must reach its trampoline, not become prose.
    EvaluationError {
        file: String,
        error: Box<crate::EvalError>,
    },

    /// The library's implementation is a compiled shared object
    /// (`include-shared`), which Patina cannot load.
    ///
    /// Distinct from `ParseError` because it is not a defect in the library
    /// or in Patina's reader: the source is well-formed and the library is
    /// simply out of reach until there is an FFI. The structured diagnostic
    /// records that fact independently of the human-readable wording.
    NativeExtensionRequired { file: String, extension: String },
}

impl LibraryError {
    /// Leave a running library load without wrapping an evaluation error or
    /// control transfer in a new Scheme condition (#425).
    pub fn into_eval_error(self) -> crate::EvalError {
        use crate::HasDiagnostic;
        let diagnostic = self.diagnostic();
        let location = self.source_location().cloned();
        match self {
            Self::EvaluationError { error, .. } => error.with_diagnostic(diagnostic),
            other => crate::EvalError::InvalidSyntax(format!("Failed to load library: {other}"))
                .with_diagnostic(diagnostic)
                .at_opt(location),
        }
    }

    pub fn source_location(&self) -> Option<&patina_core::SourceLocation> {
        match self {
            Self::ProcessingError { location, .. } => location.as_ref(),
            Self::EvaluationError { error, .. } => error.source_location(),
            _ => None,
        }
    }

    pub fn at_opt(mut self, source: Option<patina_core::SourceLocation>) -> Self {
        if let Self::ProcessingError { location, .. } = &mut self {
            *location = source;
        }
        self
    }

    pub fn processing(
        source: Option<&std::path::Path>,
        message: impl Into<String>,
        diagnostic: crate::Diagnostic,
    ) -> Self {
        Self::ProcessingError {
            file: source.map(|p| p.display().to_string()).unwrap_or_default(),
            message: message.into(),
            diagnostic: Box::new(diagnostic.in_library(source)),
            location: None,
        }
    }

    pub fn load(source: Option<&std::path::Path>, message: impl Into<String>) -> Self {
        let message = message.into();
        let diagnostic = crate::Diagnostic::new(crate::DiagnosticKind::Load, message.clone());
        Self::processing(source, message, diagnostic)
    }

    /// A library that is not loaded, looked up by `name`.
    pub fn not_found(name: &[String]) -> Self {
        LibraryError::NotFound {
            name: name.to_vec(),
            searched: Vec::new(),
        }
    }

    /// A library that none of the directories on `searched` holds.
    pub fn not_found_in(name: &[String], searched: &[PathBuf]) -> Self {
        LibraryError::NotFound {
            name: name.to_vec(),
            searched: searched.to_vec(),
        }
    }

    /// A failure while reading a library, attributed to the file
    /// it came from. `None` covers an inline `define-library`, which has no
    /// file — the empty string the callers used to write by hand.
    pub fn parse(source: Option<&std::path::Path>, message: impl Into<String>) -> Self {
        LibraryError::ParseError {
            file: source.map(|p| p.display().to_string()).unwrap_or_default(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::NotFound { name, searched } => {
                write!(f, "Library {} not found", format_library_name(name))?;
                if !searched.is_empty() {
                    let dirs: Vec<String> =
                        searched.iter().map(|d| d.display().to_string()).collect();
                    write!(f, "; searched {}", dirs.join(", "))?;
                }
                Ok(())
            }
            LibraryError::CircularDependency(chain) => {
                writeln!(f, "Circular library dependency detected:")?;
                for lib_name in chain {
                    writeln!(f, "  {} imports", format_library_name(lib_name))?;
                }
                write!(f, "  {} (circular)", format_library_name(&chain[0]))
            }
            LibraryError::AlreadyLoaded(name) => {
                write!(f, "Library {} is already loaded", format_library_name(name))
            }
            LibraryError::InvalidName(msg) => {
                write!(f, "Invalid library name: {}", msg)
            }
            LibraryError::IoError(msg) => {
                write!(f, "I/O error: {}", msg)
            }
            LibraryError::ParseError { file, message } => {
                write!(f, "Parse error in {}: {}", file, message)
            }
            LibraryError::ProcessingError { file, message, .. } => {
                write!(f, "Error loading library in {file}: {message}")
            }
            LibraryError::EvaluationError { file, error } => {
                if file.is_empty() {
                    write!(f, "Error evaluating library body: {error}")
                } else {
                    write!(f, "Error evaluating library body in {file}: {error}")
                }
            }
            LibraryError::NativeExtensionRequired { file, extension } => {
                write!(
                    f,
                    "Library in {} requires the native extension \"{}\" (include-shared), which Patina cannot load — \
                     it needs a foreign-function interface",
                    file, extension
                )
            }
        }
    }
}

impl std::error::Error for LibraryError {}

/// Format a library name for display: ["scheme", "base"] → "(scheme base)"
fn format_library_name(name: &[String]) -> String {
    format!("({})", name.join(" "))
}

/// The file extensions a library may be written in, tried in this order.
///
/// `.sld` is R7RS. `.sls` is what R6RS libraries are distributed as, and the
/// parser reads a `(library …)` form out of either — the suffix records which
/// dialect a file was written for, not which one it is allowed to contain.
const LIBRARY_FILE_EXTENSIONS: [&str; 2] = ["sld", "sls"];

/// Resolve a library name to a file under `search_paths`.
///
/// `(scheme base)` becomes `scheme/base.sld`, then `scheme/base.sls`, under
/// each search path in turn. The extension preference is applied *within* a
/// search path, so a directory holding both spellings resolves to the R7RS
/// one while an earlier search path still wins over a later one whatever it
/// holds. Returns `None` when no candidate exists.
///
/// One function rather than one per caller: this is reached both from
/// [`LibraryRegistry::find_library_file`] and from patina-frontend's
/// `SchemeLibraryLoader`, which is the resolver every actual library load goes
/// through, and the two silently disagreeing is a failure with no symptom at
/// the point of the mistake — an added extension or a changed search order
/// would simply not apply to half the callers.
///
/// The suffix is appended rather than set, so a name part containing a dot
/// keeps it.
pub fn find_library_file_in(
    fs: &dyn FileSystem,
    name: &[String],
    search_paths: &[PathBuf],
) -> Option<PathBuf> {
    let (last_part, directory_parts) = name.split_last()?;
    let candidates = LIBRARY_FILE_EXTENSIONS.map(|extension| format!("{last_part}.{extension}"));

    let mut directory = PathBuf::new();
    for part in directory_parts {
        directory.push(part);
    }

    for search_path in search_paths {
        let base = search_path.join(&directory);
        for candidate in &candidates {
            let full_path = base.join(candidate);
            if fs.is_file(&full_path) {
                return Some(full_path);
            }
        }
    }

    None
}

/// Entries of $PATINA_LIBRARY_PATH, in order. PATH-style splitting
/// (colon-separated on Unix); empty entries are skipped.
fn env_library_paths() -> Vec<PathBuf> {
    match std::env::var_os("PATINA_LIBRARY_PATH") {
        Some(joined) => std::env::split_paths(&joined)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
        None => Vec::new(),
    }
}

/// Registry for managing loaded Scheme libraries
///
/// Responsibilities:
/// - Maintain mapping of library names to loaded libraries
/// - Manage search paths for finding library files
/// - Detect and prevent circular dependencies
/// - Provide efficient library lookup
pub struct LibraryRegistry {
    /// Loaded libraries: library name → Library
    /// Uses Vec<String> as key for library names like (scheme base)
    libraries: HashMap<Vec<String>, Library>,

    /// Search paths for finding library files (.sld)
    /// Searched in order when loading a library
    search_paths: Vec<PathBuf>,

    /// The libraries being loaded, outermost first: for circular dependency
    /// detection, and the roots of each load (#677).
    loading_stack: Vec<LoadingEntry>,

    /// Virtual filesystem for file existence checks in `find_library_file`.
    fs: Arc<dyn FileSystem>,
}

/// A library on the loading stack. Once its parsed body is handed over
/// ([`Loading::hold`]), the entry holds the environment the library is built
/// in and the body forms not yet run, which nothing else reaches until the
/// library is registered: `trace_roots` roots them, so a load may collect
/// between its forms and inside them (#677).
struct LoadingEntry {
    name: Vec<String>,
    env: Option<Rc<Environment>>,
    body: VecDeque<TaggedValue>,
}

impl LibraryRegistry {
    /// Create a new empty library registry
    pub fn new() -> Self {
        Self {
            libraries: HashMap::new(),
            search_paths: Vec::new(),
            loading_stack: Vec::new(),
            fs: Arc::new(patina_core::NativeFs),
        }
    }

    /// Set the virtual filesystem used for file existence checks.
    pub fn set_fs(&mut self, fs: Arc<dyn FileSystem>) {
        self.fs = fs;
    }

    /// Create a library registry with default search paths
    ///
    /// Default search paths (in order):
    /// 1. $PATINA_LIBRARY_PATH entries (colon-separated, if set)
    /// 2. ./lib/ (relative to current directory)
    /// 3. ./.patina/lib/ (project-local dependency directory)
    /// 4. $PATINA_HOME/lib/ (if PATINA_HOME env var is set)
    /// 5. Workspace root/lib/ (by walking up from executable to find Cargo.toml workspace)
    /// 6. Executable directory/../lib/ (relative to binary)
    ///
    /// With PATINA_ISOLATED_LIBRARIES=1, only the workspace/executable roots
    /// are included. This must take effect before bootstrap loads libraries.
    pub fn with_default_paths() -> Self {
        let mut registry = Self::new();

        if !Self::isolated_paths_enabled() {
            // 1. $PATINA_LIBRARY_PATH — the conventional user override, ahead
            // of the built-in defaults the way GUILE_LOAD_PATH and
            // CHIBI_MODULE_PATH are. CLI -I flags prepend in front of even
            // these.
            for dir in env_library_paths() {
                registry.add_search_path(dir);
            }

            // 2. Current directory ./lib/
            registry.add_search_path(PathBuf::from("./lib"));

            // 3. Project-local dependency directory ./.patina/lib/ — where a
            // future fetcher drops third-party libraries (Track L; see
            // PRD/future/PACKAGE_MANAGER_DESIGN.md). Ahead of the workspace and
            // executable paths so project dependencies win over installed ones.
            registry.add_search_path(PathBuf::from("./.patina/lib"));

            // 4. $PATINA_HOME/lib/ if set
            if let Ok(patina_home) = std::env::var("PATINA_HOME") {
                let mut path = PathBuf::from(patina_home);
                path.push("lib");
                registry.add_search_path(path);
            }
        }

        // 5. Workspace root/lib/ - walk up from executable looking for workspace Cargo.toml
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(workspace_root) = Self::find_workspace_root(&exe_path)
        {
            let mut lib_path = workspace_root;
            lib_path.push("lib");
            registry.add_search_path(lib_path);
        }

        // 6. Executable directory/../lib/
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let mut lib_path = exe_dir.to_path_buf();
            lib_path.push("../lib");
            registry.add_search_path(lib_path);
        }

        registry
    }

    /// Startup setting shared by library construction and the CLI's implicit
    /// script-directory handling. Explicit paths remain available in this mode.
    pub fn isolated_paths_enabled() -> bool {
        std::env::var("PATINA_ISOLATED_LIBRARIES").as_deref() == Ok("1")
    }

    /// Find workspace root by walking up from the given path looking for Cargo.toml with [workspace]
    fn find_workspace_root(start_path: &std::path::Path) -> Option<PathBuf> {
        let mut current = start_path.to_path_buf();

        // Walk up the directory tree
        while let Some(parent) = current.parent() {
            let cargo_toml = parent.join("Cargo.toml");

            if cargo_toml.exists() {
                // Check if this Cargo.toml defines a workspace
                if let Ok(content) = std::fs::read_to_string(&cargo_toml)
                    && content.contains("[workspace]")
                {
                    return Some(parent.to_path_buf());
                }
            }

            current = parent.to_path_buf();
        }

        None
    }

    /// Add a search path for library files
    ///
    /// Paths are searched in the order they are added.
    /// Duplicate paths are ignored.
    pub fn add_search_path(&mut self, path: PathBuf) {
        let path = Self::normalize_search_path(path);
        if !self.search_paths.contains(&path) {
            self.search_paths.push(path);
        }
    }

    /// Add a search path at the front, so it is consulted before every
    /// existing entry (the CLI's `-I`). Duplicate paths are ignored.
    pub fn prepend_search_path(&mut self, path: PathBuf) {
        let path = Self::normalize_search_path(path);
        if !self.search_paths.contains(&path) {
            self.search_paths.insert(0, path);
        }
    }

    /// Canonicalize where possible; a path that does not exist yet is kept
    /// as given.
    fn normalize_search_path(path: PathBuf) -> PathBuf {
        path.canonicalize().unwrap_or(path)
    }

    /// Get all configured search paths
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Register a library in the registry
    ///
    /// This makes the library available for future imports.
    /// If a library with the same name already exists, returns an error.
    pub fn register(&mut self, library: Library) -> Result<(), LibraryError> {
        let name = library.name.clone();

        if self.libraries.contains_key(&name) {
            return Err(LibraryError::AlreadyLoaded(name));
        }

        self.libraries.insert(name, library);
        Ok(())
    }

    /// Register a library, replacing any previous library of the same name.
    ///
    /// Used by the inline `define-library` path, where re-evaluating a form
    /// (typically at the REPL) redefines the library rather than erroring.
    /// File-based loading keeps `register`'s already-loaded error.
    pub fn register_or_replace(&mut self, library: Library) {
        self.libraries.insert(library.name.clone(), library);
    }

    /// Check if a library is already loaded
    pub fn is_loaded(&self, name: &[String]) -> bool {
        self.libraries.contains_key(name)
    }

    /// Get a loaded library by name
    ///
    /// Returns None if the library is not loaded.
    /// Use `load()` to load a library if it's not already loaded.
    pub fn get(&self, name: &[String]) -> Option<&Library> {
        self.libraries.get(name)
    }

    /// Get a mutable reference to a loaded library
    pub fn get_mut(&mut self, name: &[String]) -> Option<&mut Library> {
        self.libraries.get_mut(name)
    }

    /// Find the file path for a library, over this registry's search paths.
    ///
    /// See [`find_library_file_in`], which does the work.
    pub fn find_library_file(&self, name: &[String]) -> Option<PathBuf> {
        find_library_file_in(self.fs.as_ref(), name, &self.search_paths)
    }

    /// Begin loading a library (for circular dependency detection)
    ///
    /// Call this when starting to load a library.
    /// Must be paired with `end_loading()` when done.
    ///
    /// Returns an error if this would create a circular dependency.
    pub fn begin_loading(&mut self, name: &[String]) -> Result<(), LibraryError> {
        // Check if this library is already in the loading stack
        if self.loading_stack.iter().any(|entry| entry.name == name) {
            // Circular dependency detected
            let mut chain: Vec<Vec<String>> = self
                .loading_stack
                .iter()
                .map(|entry| entry.name.clone())
                .collect();
            chain.push(name.to_vec());
            return Err(LibraryError::CircularDependency(chain));
        }

        self.loading_stack.push(LoadingEntry {
            name: name.to_vec(),
            env: None,
            body: VecDeque::new(),
        });
        Ok(())
    }

    /// End loading a library (for circular dependency detection)
    ///
    /// Call this when done loading a library, whether successful or not. What
    /// its entry rooted is let go with it.
    pub fn end_loading(&mut self, name: &[String]) {
        if let Some(pos) = self
            .loading_stack
            .iter()
            .position(|entry| entry.name == name)
        {
            self.loading_stack.remove(pos);
        }
    }

    /// The loading stack's entry for `name`.
    fn entry_mut(&mut self, name: &[String]) -> Option<&mut LoadingEntry> {
        self.loading_stack
            .iter_mut()
            .find(|entry| entry.name == name)
    }

    /// [`LibraryRegistry::begin_loading`] for as long as the returned guard
    /// lives: dropping it ends the load. A loader holds it across everything
    /// that can fail, so a failure cannot leave the library on the loading
    /// stack, where the next attempt to load it — after a `guard` caught the
    /// first failure, or at a REPL after fixing a typo — would be reported as
    /// a cycle through itself (#436).
    pub fn begin_loading_scoped(
        registry: &Rc<RefCell<Self>>,
        name: &[String],
    ) -> Result<Loading, LibraryError> {
        registry.borrow_mut().begin_loading(name)?;
        Ok(Loading {
            registry: Rc::clone(registry),
            name: name.to_vec(),
        })
    }

    /// Get all loaded library names
    pub fn loaded_libraries(&self) -> Vec<&Vec<String>> {
        self.libraries.keys().collect()
    }

    /// Iterate over every loaded library.
    pub fn iter_libraries(&self) -> impl Iterator<Item = &Library> {
        self.libraries.values()
    }

    /// Clear all loaded libraries
    ///
    /// Useful for testing or resetting the interpreter state.
    pub fn clear(&mut self) {
        self.libraries.clear();
        self.loading_stack.clear();
    }
}

impl Default for LibraryRegistry {
    fn default() -> Self {
        Self::with_default_paths()
    }
}

/// Every loaded library is a GC root: each carries both an `exports` map and
/// an environment (`docs/GC_DESIGN.md` §5.3). So is every library being
/// loaded, its environment and the body forms it has not yet run (#677).
/// Implemented here so both backends root libraries identically — the rule
/// lives with the registry rather than being restated in each backend's root
/// provider.
///
/// Every field is named (#623); pinned by the sentinel tests
/// `a_registered_library_is_a_root` and `a_library_being_loaded_is_a_root`.
impl patina_core::GcRoots for LibraryRegistry {
    fn trace_roots(&self, visitor: &mut patina_core::GcVisitor<'_>) {
        let LibraryRegistry {
            libraries,
            // Directories.
            search_paths: _,
            loading_stack,
            // The filesystem.
            fs: _,
        } = self;
        for library in libraries.values() {
            visitor.visit_library(library);
        }
        for entry in loading_stack {
            let LoadingEntry {
                // A library name.
                name: _,
                env,
                body,
            } = entry;
            if let Some(env) = env {
                visitor.visit_env(env);
            }
            for &form in body {
                visitor.visit(form);
            }
        }
    }
}

impl LibraryRegistry {
    /// Borrow the registry for rooting, or report that it is unavailable.
    ///
    /// `Ok(Some)` — borrowed, pass it as a root. `Ok(None)` — there is no
    /// registry (a temporary state), so there is nothing to root. `Err` — a
    /// library load holds it mutably, and the caller **must not collect**:
    /// libraries are a root set, so tracing without them would free live
    /// values. Lives here so that rule sits with the root provider it
    /// constrains rather than being restated in each backend's safe point.
    #[allow(clippy::result_unit_err)]
    pub fn try_roots(
        registry: Option<&std::cell::RefCell<Self>>,
    ) -> Result<Option<std::cell::Ref<'_, Self>>, ()> {
        match registry {
            Some(cell) => cell.try_borrow().map(Some).map_err(|_| ()),
            None => Ok(None),
        }
    }
}

impl crate::HasDiagnostic for LibraryError {
    fn diagnostic(&self) -> crate::Diagnostic {
        use crate::{Diagnostic, DiagnosticKind as K};
        match self {
            Self::NotFound { name, .. } => {
                let mut d = Diagnostic::new(K::MissingLibrary, self.to_string());
                d.library = Some(name.clone());
                d
            }
            Self::ParseError { file, message } => Diagnostic::new(K::Parse, message).at_path(file),
            Self::ProcessingError { diagnostic, .. } => (**diagnostic).clone(),
            Self::EvaluationError { file, error } => error
                .diagnostic()
                .in_library(Some(std::path::Path::new(file))),
            Self::NativeExtensionRequired { file, extension } => {
                let mut d = Diagnostic::new(K::NativeExtension, self.to_string()).at_path(file);
                d.extension = Some(extension.clone());
                d
            }
            _ => Diagnostic::new(K::Load, self.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = LibraryRegistry::new();
        assert!(registry.libraries.is_empty());
        assert!(registry.search_paths.is_empty());
    }

    #[test]
    fn test_default_search_paths() {
        let registry = LibraryRegistry::with_default_paths();
        assert!(!registry.search_paths.is_empty());
        // Should at least have ./lib
        assert!(registry.search_paths.iter().any(|p| p.ends_with("lib")));
    }

    #[test]
    fn test_default_paths_include_project_local_deps() {
        let registry = LibraryRegistry::with_default_paths();
        // ./lib immediately precedes the project-local dependency directory,
        // wherever ambient PATINA_LIBRARY_PATH entries shift the pair.
        let i = registry
            .search_paths
            .iter()
            .position(|p| p.ends_with(".patina/lib"))
            .expect("./.patina/lib must be a default search path");
        assert!(registry.search_paths[i - 1].ends_with("lib"));
    }

    #[test]
    fn test_prepend_search_path_goes_first() {
        let mut registry = LibraryRegistry::new();
        registry.add_search_path(PathBuf::from("/nonexistent/a"));
        registry.prepend_search_path(PathBuf::from("/nonexistent/b"));
        assert!(registry.search_paths[0].ends_with("b"));

        // Duplicates are ignored rather than moved.
        registry.prepend_search_path(PathBuf::from("/nonexistent/a"));
        assert_eq!(registry.search_paths.len(), 2);
        assert!(registry.search_paths[0].ends_with("b"));
    }

    #[test]
    fn test_register_or_replace_overwrites() {
        let mut registry = LibraryRegistry::new();
        let name = vec!["test".to_string()];
        registry.register(Library::new(name.clone())).unwrap();

        // A second registration under the same name replaces, not errors —
        // the replacement's exports are what `get` returns afterwards.
        let mut replacement = Library::new(name.clone());
        replacement.export_tagged("v".to_string(), patina_core::TaggedValue::fixnum(2));
        registry.register_or_replace(replacement);

        assert!(registry.get(&name).unwrap().exports.contains_key("v"));
    }

    /// A library defined again is replaced in the registry (#614's path),
    /// and its environment, a namespace, keeps its charge on the heap's
    /// external bytes (#615) for as long as anything holds it — here an
    /// importer's link — and gives it back when the last holder drops. That
    /// drop is the one a namespace can see outside a sweep.
    #[test]
    fn a_replaced_library_gives_its_namespace_charge_back_when_it_drops() {
        use patina_core::environment::Environment;
        use patina_core::heap::new_shared_heap;
        use std::rc::Rc;

        let heap = new_shared_heap();
        let external = || heap.borrow().external_bytes();
        let name = vec!["test".to_string()];
        let library = |value: i64| {
            let env = Rc::new(Environment::with_heap(heap.clone()));
            for i in 0..50 {
                env.define(format!("private-{i}"), patina_core::TaggedValue::fixnum(i));
            }
            env.define("v", patina_core::TaggedValue::fixnum(value));
            let mut library = Library::with_env(name.clone(), env);
            library.export_tagged("v".to_string(), patina_core::TaggedValue::fixnum(value));
            library
        };
        let mut registry = LibraryRegistry::new();
        registry.register(library(1)).unwrap();
        let importer = Environment::with_heap(heap.clone());
        assert!(
            registry
                .get(&name)
                .unwrap()
                .import_into(&importer, "v", "v")
        );
        let old = registry.get(&name).unwrap().env.charged_bytes().unwrap();

        registry.register_or_replace(library(2));
        let new = registry.get(&name).unwrap().env.charged_bytes().unwrap();
        // The importer links the old library's binding, so the old namespace
        // lives on, and stays charged, out of the registry.
        assert_eq!(external(), importer.charged_bytes().unwrap() + old + new);

        drop(importer);
        assert_eq!(external(), new);
        drop(registry);
        assert_eq!(external(), 0);
    }

    #[test]
    fn test_add_search_path() {
        let mut registry = LibraryRegistry::new();
        registry.add_search_path(PathBuf::from("/usr/local/lib/patina"));

        assert_eq!(registry.search_paths().len(), 1);
    }

    #[test]
    fn test_register_library() {
        let mut registry = LibraryRegistry::new();
        let lib = Library::new(vec!["test".to_string()]);
        let name = vec!["test".to_string()];

        assert!(!registry.is_loaded(&name));

        registry.register(lib).unwrap();

        assert!(registry.is_loaded(&name));
        assert!(registry.get(&name).is_some());
    }

    #[test]
    fn test_duplicate_registration_error() {
        let mut registry = LibraryRegistry::new();
        let lib1 = Library::new(vec!["test".to_string()]);
        let lib2 = Library::new(vec!["test".to_string()]);

        registry.register(lib1).unwrap();
        let result = registry.register(lib2);

        assert!(result.is_err());
        match result {
            Err(LibraryError::AlreadyLoaded(name)) => {
                assert_eq!(name, vec!["test"]);
            }
            _ => panic!("Expected AlreadyLoaded error"),
        }
    }

    #[test]
    fn test_library_name_to_file_path() {
        let _registry = LibraryRegistry::new();

        // Note: find_library_file returns None if file doesn't exist
        // We're just testing the path construction logic here

        // Library names map to paths:
        // (scheme base) → scheme/base.sld
        // (mylib) → mylib.sld
        // (mylib utils) → mylib/utils.sld
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut registry = LibraryRegistry::new();

        let name1 = vec!["lib1".to_string()];
        let name2 = vec!["lib2".to_string()];

        // Start loading lib1
        registry.begin_loading(&name1).unwrap();

        // Start loading lib2 (lib1 imports lib2)
        registry.begin_loading(&name2).unwrap();

        // Try to load lib1 again (lib2 imports lib1) - should fail
        let result = registry.begin_loading(&name1);
        assert!(result.is_err());

        match result {
            Err(LibraryError::CircularDependency(chain)) => {
                assert_eq!(chain.len(), 3);
                assert_eq!(chain[0], name1);
                assert_eq!(chain[1], name2);
                assert_eq!(chain[2], name1);
            }
            _ => panic!("Expected CircularDependency error"),
        }

        // Clean up
        registry.end_loading(&name2);
        registry.end_loading(&name1);
    }

    #[test]
    fn test_get_loaded_libraries() {
        let mut registry = LibraryRegistry::new();

        let lib1 = Library::new(vec!["lib1".to_string()]);
        let lib2 = Library::new(vec!["scheme".to_string(), "base".to_string()]);

        registry.register(lib1).unwrap();
        registry.register(lib2).unwrap();

        let loaded = registry.loaded_libraries();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|n| **n == vec!["lib1"]));
        assert!(loaded.iter().any(|n| **n == vec!["scheme", "base"]));
    }

    #[test]
    fn test_clear_registry() {
        let mut registry = LibraryRegistry::new();
        let lib = Library::new(vec!["test".to_string()]);
        let name = vec!["test".to_string()];

        registry.register(lib).unwrap();
        assert!(registry.is_loaded(&name));

        registry.clear();
        assert!(!registry.is_loaded(&name));
        assert!(registry.loaded_libraries().is_empty());
    }
}

/// The registry's root provider (#623): built by a struct literal, so a new
/// field breaks this test as well as `trace_roots`, with a registered
/// library whose export and environment each hold a value nothing else
/// reaches. `Library`'s own fields are pinned in patina-core
/// (`library_exports_and_environment`).
#[cfg(test)]
mod gc_root_tests {
    use super::*;
    use patina_core::Environment;
    use patina_core::heap::gc::collect_for_tests;
    use patina_core::heap::sentinels::Sentinels;

    #[test]
    fn a_registered_library_is_a_root() {
        let env = Rc::new(Environment::new());
        let shared = env.heap().clone();
        let mut heap = shared.borrow_mut();
        let mut s = Sentinels::new(&mut heap);
        let exported = s.pair(&mut heap, "LibraryRegistry.libraries: Library.exports");
        let private = s.vector(&mut heap, "LibraryRegistry.libraries: Library.env");
        drop(heap);
        env.define("private", private);

        let name = vec!["sentinel".to_string()];
        let mut library = Library::with_env(name.clone(), env);
        library.export_tagged("exported".to_string(), exported);
        let registry = LibraryRegistry {
            libraries: HashMap::from([(name, library)]),
            search_paths: Vec::new(),
            loading_stack: Vec::new(),
            fs: Arc::new(patina_core::NativeFs),
        };

        let mut heap = shared.borrow_mut();
        collect_for_tests(&mut heap, &[&registry]);
        s.assert_survived(&heap);
    }

    /// A library being loaded, every field of its entry: the environment it
    /// is built in, and a body form it has not yet run (#677).
    #[test]
    fn a_library_being_loaded_is_a_root() {
        let env = Rc::new(Environment::new());
        let shared = env.heap().clone();
        let mut heap = shared.borrow_mut();
        let mut s = Sentinels::new(&mut heap);
        let defined = s.pair(&mut heap, "LibraryRegistry.loading_stack: LoadingEntry.env");
        let form = s.vector(
            &mut heap,
            "LibraryRegistry.loading_stack: LoadingEntry.body",
        );
        drop(heap);
        env.define("defined", defined);

        let registry = LibraryRegistry {
            libraries: HashMap::new(),
            search_paths: Vec::new(),
            loading_stack: vec![LoadingEntry {
                name: vec!["sentinel".to_string()],
                env: Some(env),
                body: VecDeque::from([form]),
            }],
            fs: Arc::new(patina_core::NativeFs),
        };

        let mut heap = shared.borrow_mut();
        collect_for_tests(&mut heap, &[&registry]);
        s.assert_survived(&heap);
    }
}
