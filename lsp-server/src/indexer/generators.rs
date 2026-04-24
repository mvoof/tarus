//! Generator discovery and bindings detection

use super::types::GeneratorKind;
use super::ProjectIndex;
use std::path::Path;

impl ProjectIndex {
    /// Returns true if at least one binding file (Specta / ts-rs / typegen) has been indexed.
    ///
    /// Used to gate type-level diagnostics: type checking is only meaningful when
    /// a generated bindings file is present in the workspace.
    pub fn has_bindings_files(&self) -> bool {
        self.command_schemas.iter().any(|e| {
            matches!(
                e.value().generator,
                GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen
            )
        }) || !self.type_aliases.is_empty()
            || self.event_schemas.iter().any(|e| {
                matches!(
                    e.value().generator,
                    GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen
                )
            })
    }

    /// Replace the list of config-discovered generators.
    pub fn set_generator_bindings(&self, bindings: Vec<super::types::DiscoveredGenerator>) {
        *self.generator_bindings.write() = bindings;
    }

    /// Return the `GeneratorKind` for a given file path based on config-discovered generators.
    ///
    /// For directory-match generators the path must be inside the output directory.
    /// For exact-file generators the path must equal the output file.
    pub fn get_generator_for_file(&self, path: &Path) -> Option<GeneratorKind> {
        let bindings = self.generator_bindings.read();

        for b in bindings.iter() {
            if b.is_directory {
                if path.starts_with(&b.output_path) {
                    return Some(b.kind);
                }
            } else if path == b.output_path {
                return Some(b.kind);
            }
        }
        None
    }
}
