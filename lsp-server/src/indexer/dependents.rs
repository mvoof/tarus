//! Which files depend on which names: the reverse of what each file looked up
//! while it was parsed, and the names a re-indexed file changed.

use super::{Forwarder, ProjectIndex};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// For each name, the files that looked it up.
#[derive(Debug, Default)]
pub(crate) struct NameUsers {
    by_name: DashMap<String, HashSet<PathBuf>>,
    // Reverse index for stale removal: file -> the names it looked up
    by_file: DashMap<PathBuf, Vec<String>>,
}

impl NameUsers {
    /// Record that `path` looks up `names`, replacing what it looked up before.
    fn set(&self, path: &Path, names: HashSet<String>) {
        // Most re-parses look up exactly what the last one did.
        if self.by_file.get(path).is_some_and(|stored| {
            stored.len() == names.len() && stored.iter().all(|name| names.contains(name))
        }) {
            return;
        }

        self.remove(path);

        if names.is_empty() {
            return;
        }

        for name in &names {
            self.by_name
                .entry(name.clone())
                .or_default()
                .insert(path.to_path_buf());
        }

        self.by_file
            .insert(path.to_path_buf(), names.into_iter().collect());
    }

    fn remove(&self, path: &Path) {
        let Some((_, names)) = self.by_file.remove(path) else {
            return;
        };

        for name in names {
            if let Some(mut users) = self.by_name.get_mut(&name) {
                users.remove(path);
            }

            self.by_name.remove_if(&name, |_, users| users.is_empty());
        }
    }

    fn users_of<'a>(&self, names: impl IntoIterator<Item = &'a String>) -> HashSet<PathBuf> {
        names
            .into_iter()
            .filter_map(|name| self.by_name.get(name))
            .flat_map(|users| users.value().iter().cloned().collect::<Vec<_>>())
            .collect()
    }
}

/// The constants and forwarding helpers one file declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDeclarations {
    rust: crate::utils::RustConstants,
    js: HashMap<String, String>,
    forwarders: Vec<Forwarder>,
}

/// Names whose value may have changed, by the language that looks them up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameChanges {
    /// Rust constants.
    pub rust: HashSet<String>,
    /// JS/TS constants and forwarding helpers.
    pub frontend: HashSet<String>,
}

impl NameChanges {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rust.is_empty() && self.frontend.is_empty()
    }

    pub fn extend(&mut self, other: Self) {
        self.rust.extend(other.rust);
        self.frontend.extend(other.frontend);
    }
}

impl ProjectIndex {
    /// Record what `path` looked up, in the maps of its language.
    pub(super) fn set_name_uses(&self, path: &Path, uses: super::NameUses) {
        if path.extension().is_some_and(|ext| ext == "rs") {
            self.rust_constant_users.set(path, uses.constants);
        } else {
            self.js_constant_users.set(path, uses.constants);
            self.forwarder_users.set(path, uses.forwarders);
        }
    }

    pub(super) fn remove_name_uses(&self, path: &Path) {
        self.rust_constant_users.remove(path);
        self.js_constant_users.remove(path);
        self.forwarder_users.remove(path);
    }

    /// Files that looked up one of the `changed` names when they were parsed.
    #[must_use]
    pub fn dependents_of(&self, changed: &NameChanges) -> HashSet<PathBuf> {
        let mut files = self.rust_constant_users.users_of(&changed.rust);
        files.extend(self.js_constant_users.users_of(&changed.frontend));
        files.extend(self.forwarder_users.users_of(&changed.frontend));

        files
    }

    /// What `path` declares now; compare two with [`Self::changes_between`].
    #[must_use]
    pub fn declarations_of(&self, path: &Path) -> FileDeclarations {
        FileDeclarations {
            rust: self.rust_constants_of(path),
            js: super::owned_by(&self.js_constants, &self.js_constants_paths, path),
            forwarders: super::owned_entries(&self.forwarders, &self.forwarder_paths, path)
                .into_iter()
                .map(|(_, forwarder)| forwarder)
                .collect(),
        }
    }

    /// The names whose value can differ now that a file declares `after` instead of
    /// `before`: those it added, removed or changed, and every Rust constant whose
    /// `const A: &str = B;` chain reaches one of them.
    ///
    /// A name another file also declares is included even when that file's value
    /// is the one in use: that only re-parses a file for nothing.
    #[must_use]
    pub fn changes_between(
        &self,
        before: &FileDeclarations,
        after: &FileDeclarations,
    ) -> NameChanges {
        let mut rust = differing_keys(&before.rust.values, &after.rust.values);
        rust.extend(differing_keys(
            &before.rust.references,
            &after.rust.references,
        ));

        // A `type A = &str;` decides whether the references declared through `A`
        // are string constants at all.
        let changed_aliases: HashSet<&String> = before
            .rust
            .str_type_aliases
            .symmetric_difference(&after.rust.str_type_aliases)
            .collect();

        if !changed_aliases.is_empty() {
            rust.extend(self.references_declared_through(&changed_aliases));
        }

        let mut frontend = differing_keys(&before.js, &after.js);
        frontend.extend(differing_forwarders(&before.forwarders, &after.forwarders));

        NameChanges {
            rust: self.with_references_to(rust),
            frontend,
        }
    }

    /// Names of Rust references declared with one of `aliases` as their type.
    fn references_declared_through(&self, aliases: &HashSet<&String>) -> HashSet<String> {
        self.rust_constant_references
            .iter()
            .filter(|entry| {
                entry.value().iter().any(|(_, reference)| {
                    reference
                        .type_alias
                        .as_ref()
                        .is_some_and(|alias| aliases.contains(alias))
                })
            })
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// `names` plus every Rust constant whose reference chain reaches one of them.
    fn with_references_to(&self, mut names: HashSet<String>) -> HashSet<String> {
        // A frontend edit changes no Rust name: nothing to follow.
        if names.is_empty() {
            return names;
        }

        // One pass over the references, then outward from the changed names.
        let mut referenced_by: HashMap<String, Vec<String>> = HashMap::new();

        for entry in &self.rust_constant_references {
            for (_, reference) in entry.value() {
                referenced_by
                    .entry(reference.target.clone())
                    .or_default()
                    .push(entry.key().clone());
            }
        }

        let mut pending: Vec<String> = names.iter().cloned().collect();

        while let Some(name) = pending.pop() {
            for referrer in referenced_by.get(&name).into_iter().flatten() {
                if names.insert(referrer.clone()) {
                    pending.push(referrer.clone());
                }
            }
        }

        names
    }
}

/// Keys added, removed or given a different value between two maps.
fn differing_keys<V: PartialEq>(
    before: &HashMap<String, V>,
    after: &HashMap<String, V>,
) -> HashSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .cloned()
        .collect()
}

/// Helper names whose declarations differ: two same-named helpers make a name
/// ambiguous, so the count matters as well as the shapes.
fn differing_forwarders(before: &[Forwarder], after: &[Forwarder]) -> HashSet<String> {
    let by_name = |forwarders: &[Forwarder]| {
        let mut by_name: HashMap<String, Vec<Forwarder>> = HashMap::new();

        for forwarder in forwarders {
            by_name
                .entry(forwarder.function_name.clone())
                .or_default()
                .push(forwarder.clone());
        }

        by_name
    };
    let (before, after) = (by_name(before), by_name(after));

    before
        .keys()
        .chain(after.keys())
        .filter(|name| {
            !super::same_items(
                before.get(*name).map_or(&[][..], Vec::as_slice),
                after.get(*name).map_or(&[][..], Vec::as_slice),
            )
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{ConstantReference, RustConstants};

    fn rust_file(values: &[(&str, &str)], references: &[(&str, &str)]) -> RustConstants {
        RustConstants {
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            references: references
                .iter()
                .map(|(name, target)| {
                    (
                        (*name).to_string(),
                        ConstantReference {
                            target: (*target).to_string(),
                            type_alias: None,
                        },
                    )
                })
                .collect(),
            str_type_aliases: HashSet::new(),
        }
    }

    #[test]
    fn test_a_changed_constant_reaches_the_references_chained_to_it() {
        // `ALIAS -> MIDDLE -> BASE`, each in its own file: changing `BASE` changes
        // what both of the others stand for.
        let index = ProjectIndex::new();
        let base = Path::new("base.rs");
        index.add_rust_constants(
            PathBuf::from("middle.rs"),
            rust_file(&[], &[("MIDDLE", "BASE")]),
        );
        index.add_rust_constants(
            PathBuf::from("alias.rs"),
            rust_file(&[], &[("ALIAS", "MIDDLE")]),
        );
        index.add_rust_constants(base.to_path_buf(), rust_file(&[("BASE", "a")], &[]));

        let before = index.declarations_of(base);
        index.add_rust_constants(base.to_path_buf(), rust_file(&[("BASE", "b")], &[]));
        let changes = index.changes_between(&before, &index.declarations_of(base));

        let expected: HashSet<String> = ["BASE", "MIDDLE", "ALIAS"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(changes.rust, expected);
        assert!(changes.frontend.is_empty());
    }

    #[test]
    fn test_a_reference_cycle_ends() {
        // `A -> B -> A` never resolves, but following it must still stop.
        let index = ProjectIndex::new();
        let path = Path::new("cycle.rs");
        index.add_rust_constants(
            PathBuf::from("other.rs"),
            rust_file(&[], &[("A", "B"), ("B", "A")]),
        );
        index.add_rust_constants(path.to_path_buf(), rust_file(&[("C", "c")], &[]));

        let before = index.declarations_of(path);
        index.add_rust_constants(path.to_path_buf(), rust_file(&[("B", "b")], &[]));
        let changes = index.changes_between(&before, &index.declarations_of(path));

        let expected: HashSet<String> = ["A", "B", "C"].into_iter().map(String::from).collect();
        assert_eq!(changes.rust, expected);
    }

    #[test]
    fn test_unchanged_declarations_change_nothing() {
        let index = ProjectIndex::new();
        let path = Path::new("names.ts");
        let constants: HashMap<String, String> =
            HashMap::from([("RUN".to_string(), "run://a".to_string())]);

        index.add_js_constants(path.to_path_buf(), constants.clone());
        let before = index.declarations_of(path);
        index.add_js_constants(path.to_path_buf(), constants);

        assert!(index
            .changes_between(&before, &index.declarations_of(path))
            .is_empty());
    }

    #[test]
    fn test_dependents_are_found_by_language() {
        // A Rust file and a frontend file both look up `EVENT`; a change to the
        // Rust constant reaches only the Rust file.
        let index = ProjectIndex::new();
        let uses = super::super::NameUses {
            constants: HashSet::from(["EVENT".to_string()]),
            forwarders: HashSet::new(),
        };
        index.set_name_uses(Path::new("run.rs"), uses.clone());
        index.set_name_uses(Path::new("use.ts"), uses);

        let changes = NameChanges {
            rust: HashSet::from(["EVENT".to_string()]),
            frontend: HashSet::new(),
        };

        assert_eq!(
            index.dependents_of(&changes),
            HashSet::from([PathBuf::from("run.rs")])
        );

        index.remove_name_uses(Path::new("run.rs"));
        assert!(index.dependents_of(&changes).is_empty());
    }
}
