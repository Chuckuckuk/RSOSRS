/// Diagnostic seam so core logic can report problems without depending on any engine's logging
/// (the C# `ICoreLog`). A host wires in an implementation that forwards to its own log where a
/// core component needs to report a problem it can't just panic for.
pub trait CoreLog {
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
}
