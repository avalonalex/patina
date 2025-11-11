use patina_runtime::Value;

/// Surface syntax - Scheme code after parsing but before macro expansion and desugaring
///
/// This represents the full R7RS syntax including all derived forms.
/// The frontend will transform this into CoreExpr.
#[derive(Debug, Clone)]
pub enum SurfaceSyntax {
    /// Direct Value (from parser)
    Value(Value),

    // Note: For Phase 1, we'll just use Value directly from the parser.
    // This enum can be expanded later when we implement the frontend.
}

// For now, Surface Syntax is just a placeholder.
// The real frontend will parse source text into Value (as it does now),
// then transform Value into CoreExpr through macro expansion and desugaring.
