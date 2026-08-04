use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::Result;

use crate::errors::{ErrorCode, FabioError};

/// Static deployment order by item type.
///
/// Items of type earlier in this list are deployed before items of types later.
/// This ensures dependencies are satisfied (e.g., Lakehouses exist before Notebooks
/// that reference them, Notebooks exist before `DataPipelines` that invoke them).
///
/// This is **aligned 1:1 with fabric-cicd's `SERIAL_ITEM_PUBLISH_ORDER`**: every
/// item type that both tools share appears in the exact same relative order as
/// fabric-cicd. fabio supports a superset of item types, so the fabio-only types
/// are slotted *between* shared types (marked `fabio-only`), which never changes
/// the relative order of the shared ones. The `fabric-cicd N` comments give the
/// position of each shared type in fabric-cicd's list.
///
/// One deliberate placement: `DataAgent` binds to `GraphModel` (a fabio-only
/// data-source type) in addition to `Ontology`, so `GraphModel` (and the other
/// graph types) are slotted between `Ontology` and `DataAgent`. `DataAgent` still
/// lands in fabric-cicd's relative position (after `Ontology`, before
/// `MLExperiment`), while staying after every data source it can reference.
pub const DEPLOY_ORDER: &[&str] = &[
    "VariableLibrary",                // fabric-cicd 1
    "Warehouse",                      // fabric-cicd 2
    "WarehouseSnapshot",              // fabio-only (after Warehouse)
    "MirroredDatabase",               // fabric-cicd 3
    "MirroredAzureDatabricksCatalog", // fabio-only (mirrored family)
    "AzureDatabricksStorage",         // fabio-only (storage family)
    "Lakehouse",                      // fabric-cicd 4
    "SQLDatabase",                    // fabric-cicd 5
    "CosmosDbDatabase",               // fabio-only (database family)
    "SnowflakeDatabase",              // fabio-only (database family)
    "Environment",                    // fabric-cicd 6
    "UserDataFunction",               // fabric-cicd 7
    "Eventhouse",                     // fabric-cicd 8
    "SparkJobDefinition",             // fabric-cicd 9
    "Notebook",                       // fabric-cicd 10
    "SemanticModel",                  // fabric-cicd 11
    "Report",                         // fabric-cicd 12
    "PaginatedReport",                // fabric-cicd 13
    "Dashboard",                      // fabio-only (reports family)
    "CopyJob",                        // fabric-cicd 14
    "DataBuildToolJob",               // fabric-cicd 15
    "KQLDatabase",                    // fabric-cicd 16
    "KQLQueryset",                    // fabric-cicd 17
    "Dataflow",                       // fabric-cicd 18
    "DataPipeline",                   // fabric-cicd 19
    "Reflex",                         // fabric-cicd 20
    "Eventstream",                    // fabric-cicd 21
    "EventSchemaSet",                 // fabio-only (eventstream family)
    "KQLDashboard",                   // fabric-cicd 22
    "GraphQLApi",                     // fabric-cicd 23
    "ApacheAirflowJob",               // fabric-cicd 24
    "MountedDataFactory",             // fabric-cicd 25
    "OperationsAgent",                // fabio-only (RTI)
    "AnomalyDetector",                // fabio-only (RTI)
    "Ontology",                       // fabric-cicd 26
    "GraphModel",                     // fabio-only (after Ontology; a DataAgent data source)
    "GraphQuerySet",                  // fabio-only
    "DigitalTwinBuilder",             // fabio-only
    "DigitalTwinBuilderFlow",         // fabio-only
    "DataAgent",                      // fabric-cicd 27 (after GraphModel — a DataAgent data source)
    "MLExperiment",                   // fabric-cicd 28
    "MLModel",                        // fabio-only (after MLExperiment)
    "Map",                            // fabric-cicd 29
    "Connection",                     // fabio-only (cross-cutting, last)
    "OrgApp",                         // fabio-only
    "OrgAppAudience",                 // fabio-only
];

/// Returns the deploy priority for a given item type.
/// Lower number = deployed earlier. Unknown types get a high number (deployed last).
#[inline]
pub fn deploy_priority(item_type: &str) -> usize {
    DEPLOY_ORDER
        .iter()
        .position(|&t| t.eq_ignore_ascii_case(item_type))
        .unwrap_or(DEPLOY_ORDER.len())
}

/// Returns the dependency tier for a given item type.
///
/// Types in the same tier have no dependencies on each other and can be deployed
/// concurrently. Types in tier N depend on types in tiers 0..N-1. Tier boundaries
/// track the fabric-cicd-aligned `DEPLOY_ORDER` above.
///
/// Tiers correspond to dependency layers:
/// - Tier 0: Data storage layer (foundation)
/// - Tier 1: Compute & runtime (`Environment`, `UserDataFunction`, `Eventhouse`)
/// - Tier 2: Code & logic (`SparkJobDefinition`, `Notebook`)
/// - Tier 3: `SemanticModel` (needs refresh before reports can use it)
/// - Tier 4: Reports, jobs & `KQLDatabase` (`Report`..`KQLDatabase`)
/// - Tier 5: Reactive & streaming (`KQLQueryset`..`KQLDashboard`)
/// - Tier 6: APIs & integration (`GraphQLApi`..`MountedDataFactory`)
/// - Tier 7: RTI agents (`OperationsAgent`, `AnomalyDetector`)
/// - Tier 8: `Ontology`
/// - Tier 9: Graph (`GraphModel`..`DigitalTwinBuilderFlow`)
/// - Tier 10: `DataAgent` (binds any data source; must come after all of them)
/// - Tier 11: `MLExperiment`
/// - Tier 12: `MLModel`, visualization & cross-cutting (+ unknown)
#[inline]
pub fn deploy_tier(item_type: &str) -> usize {
    let priority = deploy_priority(item_type);
    match priority {
        0..=9 => 0,   // Storage: VariableLibrary..SnowflakeDatabase
        10..=12 => 1, // Compute: Environment, UserDataFunction, Eventhouse
        13..=14 => 2, // Code: SparkJobDefinition, Notebook
        15 => 3,      // SemanticModel (needs refresh before Reports can use it)
        16..=21 => 4, // Reports, jobs & KQLDatabase: Report..KQLDatabase
        22..=28 => 5, // Reactive: KQLQueryset..KQLDashboard
        29..=31 => 6, // APIs: GraphQLApi..MountedDataFactory
        32..=33 => 7, // RTI agents: OperationsAgent, AnomalyDetector
        34 => 8,      // Ontology
        35..=38 => 9, // Graph: GraphModel..DigitalTwinBuilderFlow
        39 => 10,     // DataAgent: binds any data source, must come after all of them
        40 => 11,     // MLExperiment
        _ => 12,      // MLModel, Map, Connection, OrgApp, OrgAppAudience + unknown
    }
}

/// Reverse deployment order for deletes.
/// Items that depend on others should be deleted first.
#[inline]
pub fn delete_priority(item_type: &str) -> usize {
    let pos = deploy_priority(item_type);
    DEPLOY_ORDER.len().saturating_sub(pos)
}

/// Topological sort for items that reference each other (e.g., sub-pipelines).
///
/// `items` is a list of (name, references) where references are names of other
/// items in the same list that must be deployed first.
///
/// Returns the sorted order, or an error if circular dependencies are detected.
pub fn topological_sort(items: &[(String, Vec<String>)]) -> Result<Vec<String>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    // Build adjacency list and in-degree count
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    let names: HashSet<&str> = items.iter().map(|(n, _)| n.as_str()).collect();

    for (name, _) in items {
        in_degree.entry(name.as_str()).or_insert(0);
        dependents.entry(name.as_str()).or_default();
    }

    for (name, refs) in items {
        for dep in refs {
            // Only count references to items within our set
            if names.contains(dep.as_str()) {
                *in_degree.entry(name.as_str()).or_insert(0) += 1;
                dependents
                    .entry(dep.as_str())
                    .or_default()
                    .push(name.as_str());
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    let mut sorted = Vec::with_capacity(items.len());

    while let Some(node) = queue.pop_front() {
        sorted.push(node.to_owned());

        if let Some(deps) = dependents.get(node) {
            for &dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }
    }

    if sorted.len() != items.len() {
        let sorted_set: std::collections::HashSet<&str> =
            sorted.iter().map(String::as_str).collect();
        let unsorted: Vec<&str> = names
            .iter()
            .filter(|n| !sorted_set.contains(*n))
            .copied()
            .collect();
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Circular dependency detected among items: {}",
                unsorted.join(", ")
            ),
            "Break the cycle by splitting pipelines into separate deploy batches or removing the circular ExecutePipeline activity reference.",
        )
        .into());
    }

    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deploy_priority_known_types() {
        assert!(deploy_priority("VariableLibrary") < deploy_priority("Notebook"));
        assert!(deploy_priority("Notebook") < deploy_priority("DataPipeline"));
        assert!(deploy_priority("Lakehouse") < deploy_priority("Notebook"));
        assert!(deploy_priority("SemanticModel") < deploy_priority("Report"));
    }

    #[test]
    fn test_deploy_priority_unknown_type() {
        let unknown = deploy_priority("UnknownType");
        assert_eq!(unknown, DEPLOY_ORDER.len());
    }

    #[test]
    fn test_data_agent_deploys_after_its_datasource_types() {
        // A DataAgent binds to these data-source types; it must deploy AFTER all
        // of them (both in linear priority and in dependency tier) so its
        // datasource references resolve to the freshly-deployed item ids.
        for src in [
            "Lakehouse",
            "Warehouse",
            "KQLDatabase",
            "SemanticModel",
            "Ontology",
            "GraphModel",
            "MirroredDatabase",
            "SQLDatabase",
        ] {
            assert!(
                deploy_priority("DataAgent") > deploy_priority(src),
                "DataAgent must have higher priority than {src}"
            );
            assert!(
                deploy_tier("DataAgent") > deploy_tier(src),
                "DataAgent must be in a later tier than {src}"
            );
        }
    }

    // ── deploy_tier ─────────────────────────────────────────────────────────

    #[test]
    fn test_deploy_tier_storage_types_same_tier() {
        assert_eq!(deploy_tier("Warehouse"), deploy_tier("Lakehouse"));
        assert_eq!(deploy_tier("Lakehouse"), deploy_tier("SQLDatabase"));
        assert_eq!(deploy_tier("SQLDatabase"), deploy_tier("VariableLibrary"));
        assert_eq!(deploy_tier("Lakehouse"), 0);
    }

    #[test]
    fn test_deploy_tier_compute_types_same_tier() {
        assert_eq!(deploy_tier("Environment"), deploy_tier("Eventhouse"));
        assert_eq!(deploy_tier("Eventhouse"), deploy_tier("UserDataFunction"));
        assert_eq!(deploy_tier("Eventhouse"), 1);
    }

    #[test]
    fn test_deploy_tier_kql_database_after_eventhouse() {
        // KQLDatabase depends on Eventhouse (parent container), must be in a later tier
        assert!(deploy_tier("Eventhouse") < deploy_tier("KQLDatabase"));
        assert_eq!(deploy_tier("KQLDatabase"), 4);
    }

    #[test]
    fn test_deploy_tier_ordering() {
        // Aligned-order dependency layering:
        // Storage (0) < Compute (1) < Code (2) < SemanticModel (3) < Reports/KQLDatabase (4)
        assert!(deploy_tier("Lakehouse") < deploy_tier("Eventhouse"));
        assert!(deploy_tier("Eventhouse") < deploy_tier("KQLDatabase"));
        assert!(deploy_tier("Notebook") < deploy_tier("SemanticModel"));
        assert!(deploy_tier("SemanticModel") < deploy_tier("KQLDatabase"));
        assert!(deploy_tier("SemanticModel") < deploy_tier("DataPipeline"));
        // Ontology (8) < Graph (9) < DataAgent (10)
        assert!(deploy_tier("Ontology") < deploy_tier("GraphModel"));
        assert!(deploy_tier("GraphModel") < deploy_tier("DataAgent"));
    }

    #[test]
    fn test_deploy_tier_unknown_type_in_last_tier() {
        assert_eq!(deploy_tier("UnknownType"), 12);
    }

    #[test]
    fn test_shared_types_match_fabric_cicd_order() {
        // The item types fabio shares with fabric-cicd must appear in the exact
        // same relative order as fabric-cicd's SERIAL_ITEM_PUBLISH_ORDER. fabio-only
        // types are interleaved but must never reorder the shared ones.
        const FABRIC_CICD_ORDER: &[&str] = &[
            "VariableLibrary",
            "Warehouse",
            "MirroredDatabase",
            "Lakehouse",
            "SQLDatabase",
            "Environment",
            "UserDataFunction",
            "Eventhouse",
            "SparkJobDefinition",
            "Notebook",
            "SemanticModel",
            "Report",
            "PaginatedReport",
            "CopyJob",
            "DataBuildToolJob",
            "KQLDatabase",
            "KQLQueryset",
            "Dataflow",
            "DataPipeline",
            "Reflex",
            "Eventstream",
            "KQLDashboard",
            "GraphQLApi",
            "ApacheAirflowJob",
            "MountedDataFactory",
            "Ontology",
            "DataAgent",
            "MLExperiment",
            "Map",
        ];
        let shared: Vec<&str> = DEPLOY_ORDER
            .iter()
            .copied()
            .filter(|t| FABRIC_CICD_ORDER.contains(t))
            .collect();
        assert_eq!(
            shared, FABRIC_CICD_ORDER,
            "shared item types must follow fabric-cicd's relative order exactly"
        );
    }

    #[test]
    fn test_delete_priority_reverses_order() {
        assert!(delete_priority("DataPipeline") < delete_priority("Notebook"));
        assert!(delete_priority("Notebook") < delete_priority("Lakehouse"));
    }

    #[test]
    fn test_topological_sort_simple() {
        let items = vec![
            ("C".to_owned(), vec!["A".to_owned(), "B".to_owned()]),
            ("A".to_owned(), vec![]),
            ("B".to_owned(), vec!["A".to_owned()]),
        ];
        let sorted = topological_sort(&items).unwrap();
        let pos_a = sorted.iter().position(|n| n == "A").unwrap();
        let pos_b = sorted.iter().position(|n| n == "B").unwrap();
        let pos_c = sorted.iter().position(|n| n == "C").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_topological_sort_circular() {
        let items = vec![
            ("A".to_owned(), vec!["B".to_owned()]),
            ("B".to_owned(), vec!["A".to_owned()]),
        ];
        assert!(topological_sort(&items).is_err());
    }

    #[test]
    fn test_topological_sort_empty() {
        let items: Vec<(String, Vec<String>)> = vec![];
        let sorted = topological_sort(&items).unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_topological_sort_external_refs_ignored() {
        // References to items NOT in the set are ignored (not an error)
        let items = vec![
            ("A".to_owned(), vec!["External".to_owned()]),
            ("B".to_owned(), vec!["A".to_owned()]),
        ];
        let sorted = topological_sort(&items).unwrap();
        let pos_a = sorted.iter().position(|n| n == "A").unwrap();
        let pos_b = sorted.iter().position(|n| n == "B").unwrap();
        assert!(pos_a < pos_b);
    }

    #[test]
    fn test_deploy_order_entry_count() {
        // Guard against accidental additions/removals — update this if DEPLOY_ORDER changes
        assert_eq!(
            DEPLOY_ORDER.len(),
            46,
            "DEPLOY_ORDER should have 46 entries; update this test if intentionally changed"
        );
    }

    #[test]
    fn test_deploy_order_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for entry in DEPLOY_ORDER {
            assert!(
                seen.insert(*entry),
                "Duplicate entry in DEPLOY_ORDER: {entry}"
            );
        }
    }
}
