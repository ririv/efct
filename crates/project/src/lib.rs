use std::collections::{BTreeMap, BTreeSet};

pub type DependencyGraph = BTreeMap<String, BTreeSet<String>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyGraphError {
    Cycle { module: String },
    MissingNode { module: String },
}

pub fn validate_dependency_graph(graph: &DependencyGraph) -> Result<(), DependencyGraphError> {
    fn visit(
        name: &str,
        graph: &DependencyGraph,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), DependencyGraphError> {
        if visiting.contains(name) {
            return Err(DependencyGraphError::Cycle {
                module: name.to_owned(),
            });
        }
        if !visited.insert(name.to_owned()) {
            return Ok(());
        }
        let dependencies = graph
            .get(name)
            .ok_or_else(|| DependencyGraphError::MissingNode {
                module: name.to_owned(),
            })?;
        visiting.insert(name.to_owned());
        for dependency in dependencies {
            if !graph.contains_key(dependency) {
                return Err(DependencyGraphError::MissingNode {
                    module: dependency.clone(),
                });
            }
            visit(dependency, graph, visiting, visited)?;
        }
        visiting.remove(name);
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for name in graph.keys() {
        visit(name, graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cyclic_dependencies() {
        let graph = BTreeMap::from([
            ("a".to_owned(), BTreeSet::from(["b".to_owned()])),
            ("b".to_owned(), BTreeSet::from(["a".to_owned()])),
        ]);
        assert!(matches!(
            validate_dependency_graph(&graph),
            Err(DependencyGraphError::Cycle { .. })
        ));
    }
}
