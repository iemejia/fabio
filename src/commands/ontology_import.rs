//! OWL/RDF ontology importer — converts RDF/XML or JSON-LD to Fabric Ontology format.
//!
//! Compatible with the [Ontology Playground](https://github.com/microsoft/Ontology-Playground)
//! catalogue `.rdf` files. Parses `owl:Class`, `owl:DatatypeProperty`, and `owl:ObjectProperty`
//! into Fabric `EntityTypes` and `RelationshipTypes`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

// ─── Public Entry Point ──────────────────────────────────────────────────────

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn import_owl(
    cli: &Cli,
    client: &FabricClient,
    workspace: Option<&str>,
    id: Option<&str>,
    file: &str,
    output_dir: Option<&str>,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
    schema: Option<&str>,
    bindings: Option<&str>,
) -> Result<()> {
    // Validate arguments
    if workspace.is_some() && id.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--id is required when --workspace is specified.".to_string(),
            "Example: fabio ontology import --workspace <WS> --id <ID> --file ontology.rdf"
                .to_string(),
        )
        .into());
    }
    if workspace.is_none() && output_dir.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --workspace (to push to Fabric) or --output-dir (to export locally) must be provided.".to_string(),
            "Example: fabio ontology import --file ontology.rdf --output-dir ./fabric-ontology/"
                .to_string(),
        )
        .into());
    }

    // Read and parse the file
    let content = fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Failed to read file '{file}': {e}"))?;

    let ext = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Detect format: prefer file extension, fall back to content sniffing
    let format = match ext.as_str() {
        "rdf" | "owl" | "xml" => "rdf",
        "jsonld" | "json" => "jsonld",
        _ => {
            // No recognized extension — detect from content
            let trimmed = content.trim_start();
            if trimmed.starts_with('<') {
                "rdf"
            } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
                "jsonld"
            } else {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Cannot detect format for file '{file}'"),
                    "Supported formats: .rdf, .owl, .xml (RDF/XML) or .jsonld, .json (JSON-LD). \
                     Alternatively, ensure the file content starts with '<' (XML) or '{{' (JSON)."
                        .to_string(),
                )
                .into());
            }
        }
    };

    let model = match format {
        "rdf" => parse_rdf_xml(&content),
        _ => parse_json_ld(&content)?,
    };

    // Resolve optional Lakehouse binding context. Bindings turn a bare schema
    // import into a queryable graph by generating DataBindings/Contextualizations.
    let binding =
        resolve_binding_context(workspace, lakehouse, lakehouse_workspace, schema, bindings)?;

    // Convert to Fabric definition parts
    let parts = generate_fabric_parts(&model, binding.as_ref())?;

    let binding_count = parts
        .iter()
        .filter(|p| p.path.contains("DataBindings") || p.path.contains("Contextualizations"))
        .count();

    if output::dry_run_guard(
        cli,
        "ontology import",
        &serde_json::json!({
            "file": file,
            "format": ext,
            "entity_types": model.classes.len(),
            "relationship_types": model.object_properties.len(),
            "total_properties": model.datatype_properties.len(),
            "bindings": binding_count,
        }),
    ) {
        return Ok(());
    }

    // Export to directory if requested
    if let Some(dir) = output_dir {
        write_to_directory(dir, &model, &parts)?;
    }

    // Push to Fabric if workspace+id provided
    if let (Some(ws), Some(ont_id)) = (workspace, id) {
        push_to_fabric(cli, client, ws, ont_id, &parts).await?;
    } else if output_dir.is_some() {
        // Only exported locally
        let obj = serde_json::json!({
            "status": "exported",
            "output_dir": output_dir,
            "entity_types": model.classes.len(),
            "relationship_types": model.object_properties.len(),
            "bindings": binding_count,
        });
        output::render_object(cli, &obj, "status");
    }

    Ok(())
}

// ─── Export (Fabric → OWL) ───────────────────────────────────────────────────

/// Fetch a Fabric Ontology definition and export it as OWL RDF/XML or JSON-LD.
pub async fn export_owl(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: &str,
    output_file: Option<&str>,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology export", "Contributor"))?;

    let model = fabric_definition_to_model(&data)?;
    let output_content = match format {
        "jsonld" => serialize_to_jsonld(&model),
        _ => serialize_to_rdf_xml(&model),
    };

    if let Some(path) = output_file {
        fs::write(path, &output_content)
            .map_err(|e| anyhow::anyhow!("Failed to write file '{path}': {e}"))?;
        let obj = serde_json::json!({
            "status": "exported",
            "file": path,
            "format": format,
            "entity_types": model.classes.len(),
            "relationship_types": model.object_properties.len(),
            "properties": model.datatype_properties.len(),
        });
        output::render_object(cli, &obj, "status");
    } else {
        print!("{output_content}");
    }
    Ok(())
}

fn fabric_definition_to_model(data: &Value) -> Result<OwlModel> {
    let parts = data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("No definition parts found"))?;

    let mut model = OwlModel::default();
    let mut entity_id_to_uri: HashMap<String, String> = HashMap::new();

    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        if !path.contains("EntityTypes") || !path.ends_with("definition.json") {
            continue;
        }
        let payload = part.get("payload").and_then(Value::as_str).unwrap_or("");
        let decoded = BASE64.decode(payload).unwrap_or_default();
        let entity: Value =
            serde_json::from_str(&String::from_utf8_lossy(&decoded)).unwrap_or_default();

        let eid = entity.get("id").and_then(Value::as_str).unwrap_or("");
        let name = entity.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let uri = format!("http://fabric.microsoft.com/ontology/{name}");
        entity_id_to_uri.insert(eid.to_string(), uri.clone());
        model.classes.push(OwlClass {
            uri: uri.clone(),
            label: name.to_string(),
        });

        let id_parts: Vec<&str> = entity
            .get("entityIdParts")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_str).collect());

        if let Some(props) = entity.get("properties").and_then(Value::as_array) {
            for prop in props {
                let pid = prop.get("id").and_then(Value::as_str).unwrap_or("");
                let pname = prop.get("name").and_then(Value::as_str).unwrap_or("");
                let vtype = prop
                    .get("valueType")
                    .and_then(Value::as_str)
                    .unwrap_or("String");
                model.datatype_properties.push(OwlDatatypeProperty {
                    label: pname.to_string(),
                    domain_uri: uri.clone(),
                    property_type: vtype.to_string(),
                    is_identifier: id_parts.contains(&pid),
                });
            }
        }
    }

    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        if !path.contains("RelationshipTypes") || !path.ends_with("definition.json") {
            continue;
        }
        let payload = part.get("payload").and_then(Value::as_str).unwrap_or("");
        let decoded = BASE64.decode(payload).unwrap_or_default();
        let rel: Value =
            serde_json::from_str(&String::from_utf8_lossy(&decoded)).unwrap_or_default();

        let name = rel.get("name").and_then(Value::as_str).unwrap_or("");
        let src = rel
            .get("source")
            .and_then(|s| s.get("entityTypeId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let tgt = rel
            .get("target")
            .and_then(|t| t.get("entityTypeId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let d = entity_id_to_uri.get(src).cloned().unwrap_or_default();
        let r = entity_id_to_uri.get(tgt).cloned().unwrap_or_default();
        if !d.is_empty() && !r.is_empty() {
            model.object_properties.push(OwlObjectProperty {
                label: name.to_string(),
                domain_uri: d,
                range_uri: r,
            });
        }
    }
    Ok(model)
}

#[allow(clippy::too_many_lines, clippy::write_with_newline)]
fn serialize_to_rdf_xml(model: &OwlModel) -> String {
    use std::fmt::Write;
    let base = "http://fabric.microsoft.com/ontology/";
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rdf:RDF\n");
    let _ = write!(s, "    xml:base=\"{base}\"\n");
    s.push_str("    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\n");
    s.push_str("    xmlns:rdfs=\"http://www.w3.org/2000/01/rdf-schema#\"\n");
    s.push_str("    xmlns:owl=\"http://www.w3.org/2002/07/owl#\"\n");
    s.push_str("    xmlns:xsd=\"http://www.w3.org/2001/XMLSchema#\"\n");
    let _ = write!(s, "    xmlns:ont=\"{base}\">\n\n");
    for c in &model.classes {
        let _ = write!(
            s,
            "    <owl:Class rdf:about=\"{}\">\n        <rdfs:label>{}</rdfs:label>\n    </owl:Class>\n\n",
            c.uri, c.label
        );
    }
    for p in &model.datatype_properties {
        let xsd = fabric_type_to_xsd(&p.property_type);
        let _ = write!(
            s,
            "    <owl:DatatypeProperty rdf:about=\"{base}{}_{}\">",
            uri_local_name(&p.domain_uri).to_lowercase(),
            p.label
        );
        let _ = write!(s, "\n        <rdfs:label>{}</rdfs:label>", p.label);
        let _ = write!(
            s,
            "\n        <rdfs:domain rdf:resource=\"{}\"/>",
            p.domain_uri
        );
        let _ = write!(
            s,
            "\n        <rdfs:range rdf:resource=\"http://www.w3.org/2001/XMLSchema#{xsd}\"/>"
        );
        if p.is_identifier {
            s.push_str("\n        <ont:isIdentifier rdf:datatype=\"http://www.w3.org/2001/XMLSchema#boolean\">true</ont:isIdentifier>");
        }
        let _ = write!(
            s,
            "\n        <ont:propertyType>{}</ont:propertyType>",
            p.property_type.to_lowercase()
        );
        s.push_str("\n    </owl:DatatypeProperty>\n\n");
    }
    for r in &model.object_properties {
        let _ = write!(
            s,
            "    <owl:ObjectProperty rdf:about=\"{base}{}\">\n",
            r.label
        );
        let _ = write!(s, "        <rdfs:label>{}</rdfs:label>\n", r.label);
        let _ = write!(
            s,
            "        <rdfs:domain rdf:resource=\"{}\"/>\n",
            r.domain_uri
        );
        let _ = write!(
            s,
            "        <rdfs:range rdf:resource=\"{}\"/>\n",
            r.range_uri
        );
        s.push_str("    </owl:ObjectProperty>\n\n");
    }
    s.push_str("</rdf:RDF>\n");
    s
}

fn serialize_to_jsonld(model: &OwlModel) -> String {
    let mut graph: Vec<Value> = Vec::new();
    for c in &model.classes {
        graph.push(serde_json::json!({"@id": c.uri, "@type": "owl:Class", "rdfs:label": c.label}));
    }
    for p in &model.datatype_properties {
        let xsd = fabric_type_to_xsd(&p.property_type);
        let mut node = serde_json::json!({
            "@id": format!("{}#{}", p.domain_uri, p.label),
            "@type": "owl:DatatypeProperty",
            "rdfs:label": p.label,
            "rdfs:domain": {"@id": &p.domain_uri},
            "rdfs:range": {"@id": format!("http://www.w3.org/2001/XMLSchema#{xsd}")},
            "ont:propertyType": p.property_type.to_lowercase(),
        });
        if p.is_identifier {
            node["ont:isIdentifier"] = serde_json::json!(true);
        }
        graph.push(node);
    }
    for r in &model.object_properties {
        graph.push(serde_json::json!({
            "@id": format!("http://fabric.microsoft.com/ontology/{}", r.label),
            "@type": "owl:ObjectProperty",
            "rdfs:label": r.label,
            "rdfs:domain": {"@id": &r.domain_uri},
            "rdfs:range": {"@id": &r.range_uri},
        }));
    }
    let doc = serde_json::json!({
        "@context": {"owl": "http://www.w3.org/2002/07/owl#", "rdfs": "http://www.w3.org/2000/01/rdf-schema#", "xsd": "http://www.w3.org/2001/XMLSchema#", "ont": "http://fabric.microsoft.com/ontology/"},
        "@graph": graph
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

fn fabric_type_to_xsd(t: &str) -> &str {
    match t {
        "BigInt" => "integer",
        "Double" => "decimal",
        "Boolean" => "boolean",
        "DateTime" => "dateTime",
        _ => "string",
    }
}

// ─── Public API for cross-module use ─────────────────────────────────────────

/// Public model struct for building OWL models externally (e.g., from context tenant).
pub struct OwlModelBuilder {
    pub classes: Vec<(String, String)>, // (uri, label)
    pub properties: Vec<(String, String, String, bool)>, // (label, domain_uri, type, is_id)
    pub relationships: Vec<(String, String, String)>, // (label, domain_uri, range_uri)
}

/// Serialize an externally-built OWL model to RDF/XML.
pub fn serialize_rdf_xml_from_model(builder: &OwlModelBuilder) -> String {
    let model = OwlModel {
        classes: builder
            .classes
            .iter()
            .map(|(uri, label)| OwlClass {
                uri: uri.clone(),
                label: label.clone(),
            })
            .collect(),
        datatype_properties: builder
            .properties
            .iter()
            .map(|(label, domain, ptype, is_id)| OwlDatatypeProperty {
                label: label.clone(),
                domain_uri: domain.clone(),
                property_type: ptype.clone(),
                is_identifier: *is_id,
            })
            .collect(),
        object_properties: builder
            .relationships
            .iter()
            .map(|(label, domain, range)| OwlObjectProperty {
                label: label.clone(),
                domain_uri: domain.clone(),
                range_uri: range.clone(),
            })
            .collect(),
    };
    serialize_to_rdf_xml(&model)
}

// ─── Data Model ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct OwlModel {
    classes: Vec<OwlClass>,
    datatype_properties: Vec<OwlDatatypeProperty>,
    object_properties: Vec<OwlObjectProperty>,
}

#[derive(Debug)]
struct OwlClass {
    uri: String,
    label: String,
}

#[derive(Debug)]
struct OwlDatatypeProperty {
    label: String,
    domain_uri: String,
    property_type: String,
    is_identifier: bool,
}

#[derive(Debug)]
struct OwlObjectProperty {
    label: String,
    domain_uri: String,
    range_uri: String,
}

// ─── RDF/XML Parser ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn parse_rdf_xml(content: &str) -> OwlModel {
    let mut model = OwlModel::default();
    let mut reader = Reader::from_str(content);

    // State tracking for current element being parsed
    let mut in_class = false;
    let mut in_datatype_prop = false;
    let mut in_object_prop = false;
    let mut current_uri = String::new();
    let mut current_label = String::new();
    let mut current_domain = String::new();
    let mut current_range = String::new();
    let mut current_prop_type = String::new();
    let mut current_is_id = false;
    let mut reading_label = false;
    let mut reading_prop_type = false;
    let mut reading_is_id = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let local_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                #[allow(clippy::collapsible_match)]
                match local_name.as_str() {
                    "Class" => {
                        in_class = true;
                        current_uri = extract_rdf_about(e);
                        current_label.clear();
                    }
                    "DatatypeProperty" => {
                        in_datatype_prop = true;
                        current_uri = extract_rdf_about(e);
                        current_label.clear();
                        current_domain.clear();
                        current_range.clear();
                        current_prop_type.clear();
                        current_is_id = false;
                    }
                    "ObjectProperty" => {
                        in_object_prop = true;
                        current_uri = extract_rdf_about(e);
                        current_label.clear();
                        current_domain.clear();
                        current_range.clear();
                    }
                    "label" => reading_label = true,
                    "propertyType" => reading_prop_type = true,
                    "isIdentifier" => reading_is_id = true,
                    "domain" => {
                        if in_datatype_prop || in_object_prop {
                            current_domain = extract_rdf_resource(e);
                        }
                    }
                    "range" => {
                        if in_datatype_prop || in_object_prop {
                            current_range = extract_rdf_resource(e);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if reading_label {
                    current_label = text;
                } else if reading_prop_type {
                    current_prop_type = text;
                } else if reading_is_id {
                    current_is_id = text == "true";
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local_name.as_str() {
                    "Class" => {
                        if in_class && !current_uri.is_empty() {
                            model.classes.push(OwlClass {
                                uri: current_uri.clone(),
                                label: {
                                    let cleaned = clean_label(&current_label);
                                    if cleaned.is_empty() {
                                        uri_local_name(&current_uri)
                                    } else {
                                        cleaned
                                    }
                                },
                            });
                        }
                        in_class = false;
                    }
                    "DatatypeProperty" => {
                        if in_datatype_prop && !current_domain.is_empty() {
                            model.datatype_properties.push(OwlDatatypeProperty {
                                label: {
                                    let cleaned = clean_label(&current_label);
                                    if cleaned.is_empty() {
                                        uri_local_name(&current_uri)
                                    } else {
                                        cleaned
                                    }
                                },
                                domain_uri: current_domain.clone(),
                                property_type: if current_prop_type.is_empty() {
                                    xsd_to_fabric_type(&current_range)
                                } else {
                                    playground_type_to_fabric(&current_prop_type)
                                },
                                is_identifier: current_is_id,
                            });
                        }
                        in_datatype_prop = false;
                    }
                    "ObjectProperty" => {
                        if in_object_prop && !current_domain.is_empty() && !current_range.is_empty()
                        {
                            model.object_properties.push(OwlObjectProperty {
                                label: {
                                    let cleaned = clean_label(&current_label);
                                    if cleaned.is_empty() {
                                        uri_local_name(&current_uri)
                                    } else {
                                        cleaned
                                    }
                                },
                                domain_uri: current_domain.clone(),
                                range_uri: current_range.clone(),
                            });
                        }
                        in_object_prop = false;
                    }
                    "label" => reading_label = false,
                    "propertyType" => reading_prop_type = false,
                    "isIdentifier" => reading_is_id = false,
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }

    model
}

fn extract_rdf_about(e: &quick_xml::events::BytesStart<'_>) -> String {
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        if key.ends_with("about") || key == "rdf:about" {
            return String::from_utf8_lossy(&attr.value).to_string();
        }
    }
    String::new()
}

fn extract_rdf_resource(e: &quick_xml::events::BytesStart<'_>) -> String {
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        if key.ends_with("resource") || key == "rdf:resource" {
            return String::from_utf8_lossy(&attr.value).to_string();
        }
    }
    String::new()
}

fn uri_local_name(uri: &str) -> String {
    uri.rsplit_once('#')
        .or_else(|| uri.rsplit_once('/'))
        .map_or_else(|| uri.to_string(), |(_, name)| name.to_string())
}

/// Normalize an `rdfs:label`: trim and collapse internal whitespace. RDF/XML
/// commonly indents label text, leaving leading/trailing/newline whitespace
/// that would otherwise leak into Fabric type names and break binding lookups.
fn clean_label(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn xsd_to_fabric_type(xsd_uri: &str) -> String {
    let local = uri_local_name(xsd_uri).to_lowercase();
    match local.as_str() {
        "integer" | "int" | "long" => "BigInt",
        "decimal" | "double" | "float" => "Double",
        "boolean" | "bool" => "Boolean",
        "date" | "datetime" | "datetimestamp" => "DateTime",
        _ => "String",
    }
    .to_string()
}

fn playground_type_to_fabric(prop_type: &str) -> String {
    match prop_type.to_lowercase().as_str() {
        "integer" | "int" => "BigInt",
        "decimal" | "double" | "float" => "Double",
        "boolean" | "bool" => "Boolean",
        "date" | "datetime" => "DateTime",
        // "string", "enum", and everything else → String
        _ => "String",
    }
    .to_string()
}

// ─── JSON-LD Parser ──────────────────────────────────────────────────────────

fn parse_json_ld(content: &str) -> Result<OwlModel> {
    let root: Value = serde_json::from_str(content)?;

    // Handle {"data": {...}} envelope from fabio context tenant
    let data = root.get("data").unwrap_or(&root);

    let graph = data
        .get("@graph")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("JSON-LD must have an @graph array"))?;

    let mut model = OwlModel::default();

    for node in graph {
        let node_type = node.get("@type").and_then(Value::as_str).unwrap_or("");
        let node_id = node.get("@id").and_then(Value::as_str).unwrap_or("");
        let label = clean_label(
            node.get("rdfs:label")
                .or_else(|| node.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        );

        match node_type {
            "owl:Class" => {
                model.classes.push(OwlClass {
                    uri: node_id.to_string(),
                    label: if label.is_empty() {
                        uri_local_name(node_id)
                    } else {
                        label
                    },
                });
            }
            "owl:DatatypeProperty" => {
                let domain = node
                    .get("rdfs:domain")
                    .and_then(|d| d.get("@id").and_then(Value::as_str).or_else(|| d.as_str()))
                    .unwrap_or("")
                    .to_string();
                let range = node
                    .get("rdfs:range")
                    .and_then(|r| r.get("@id").and_then(Value::as_str).or_else(|| r.as_str()))
                    .unwrap_or("")
                    .to_string();
                let is_id = node
                    .get("ont:isIdentifier")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                if !domain.is_empty() {
                    model.datatype_properties.push(OwlDatatypeProperty {
                        label: if label.is_empty() {
                            uri_local_name(node_id)
                        } else {
                            label
                        },
                        domain_uri: domain,
                        property_type: xsd_to_fabric_type(&range),
                        is_identifier: is_id,
                    });
                }
            }
            "owl:ObjectProperty" => {
                let domain = node
                    .get("rdfs:domain")
                    .and_then(|d| d.get("@id").and_then(Value::as_str).or_else(|| d.as_str()))
                    .unwrap_or("")
                    .to_string();
                let range = node
                    .get("rdfs:range")
                    .and_then(|r| r.get("@id").and_then(Value::as_str).or_else(|| r.as_str()))
                    .unwrap_or("")
                    .to_string();

                if !domain.is_empty() && !range.is_empty() {
                    model.object_properties.push(OwlObjectProperty {
                        label: if label.is_empty() {
                            uri_local_name(node_id)
                        } else {
                            label
                        },
                        domain_uri: domain,
                        range_uri: range,
                    });
                }
            }
            _ => {
                // For non-standard JSON-LD (like fabio context tenant output),
                // treat typed nodes as classes
                if !node_type.is_empty() && node_type != "fabric:Workspace" {
                    let clean_type = node_type.replace("fabric:", "");
                    // Only add the type if we haven't seen it
                    if !model.classes.iter().any(|c| c.label == clean_type) {
                        model.classes.push(OwlClass {
                            uri: format!("urn:fabric:type:{clean_type}"),
                            label: clean_type,
                        });
                    }
                }
            }
        }
    }

    Ok(model)
}

// ─── Lakehouse / Eventhouse Binding Model ────────────────────────────────────
//
// Shapes below mirror the official Fabric Ontology JSON schemas:
//   .../item/ontology/dataBinding/1.0.0/schema.json
//   .../item/ontology/contextualization/1.0.0/schema.json
// A DataBinding may source from a LakehouseTable (NonTimeSeries or TimeSeries)
// or an Eventhouse KustoTable (TimeSeries only). Relationship Contextualizations
// bind only to LakehouseTable, with array-valued (composite) key refs.

/// Deterministic namespace for binding/contextualization UUIDs so repeated
/// imports are idempotent (stable IDs across runs → update-in-place, not dupes).
const BINDING_NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([
    0xa1, 0x1c, 0x93, 0x69, 0x43, 0x95, 0x5a, 0x84, 0x94, 0x02, 0xd0, 0x38, 0xc8, 0xb0, 0xb2, 0x25,
]);

/// Optional binding map (JSON) that overrides table/column names, selects the
/// data source (Lakehouse/Eventhouse), and supplies relationship key columns.
#[derive(Debug, Default, serde::Deserialize)]
struct BindingSpec {
    /// Default data source applied to every entity/relationship unless overridden.
    #[serde(default)]
    source: Option<SourceSpec>,
    #[serde(default)]
    entities: HashMap<String, EntityBindingSpec>,
    #[serde(default)]
    relationships: HashMap<String, RelationshipBindingSpec>,
}

/// A data source (Lakehouse table or Eventhouse/Kusto table). Every field is
/// optional so a global `source` and per-item overrides can be merged; missing
/// coordinates fall back to CLI flags.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceSpec {
    /// `LakehouseTable` (default) or `KustoTable`.
    #[serde(rename = "type", default)]
    source_type: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
    /// `LakehouseTable` only.
    #[serde(default)]
    source_schema: Option<String>,
    /// `KustoTable` only.
    #[serde(default)]
    cluster_uri: Option<String>,
    /// `KustoTable` only.
    #[serde(default)]
    database_name: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntityBindingSpec {
    #[serde(default)]
    table: Option<String>,
    /// `NonTimeSeries` (default) or `TimeSeries`.
    #[serde(default)]
    data_binding_type: Option<String>,
    /// Required when `dataBindingType` is `TimeSeries`.
    #[serde(default)]
    timestamp_column: Option<String>,
    /// Per-entity data source override.
    #[serde(default)]
    source: Option<SourceSpec>,
    /// Maps ontology property label → source column name.
    #[serde(default)]
    columns: HashMap<String, String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelationshipBindingSpec {
    table: String,
    /// Per-relationship source override (must resolve to a `LakehouseTable`).
    #[serde(default)]
    source: Option<SourceSpec>,
    /// Source-side key columns, one per `entityIdParts` entry of the source entity.
    #[serde(default)]
    source_columns: Vec<String>,
    /// Target-side key columns, one per `entityIdParts` entry of the target entity.
    #[serde(default)]
    target_columns: Vec<String>,
}

/// Fully resolved data source for a single binding.
#[derive(Debug, Clone)]
enum ResolvedSource {
    Lakehouse {
        workspace_id: String,
        item_id: String,
        source_schema: Option<String>,
    },
    Kusto {
        workspace_id: String,
        item_id: String,
        cluster_uri: String,
        database_name: String,
    },
}

impl ResolvedSource {
    const fn is_lakehouse(&self) -> bool {
        matches!(self, Self::Lakehouse { .. })
    }

    fn table_properties(&self, table: String) -> SourceTableProperties {
        match self {
            Self::Lakehouse {
                workspace_id,
                item_id,
                source_schema,
            } => SourceTableProperties::Lakehouse(LakehouseTableProperties {
                source_type: "LakehouseTable",
                workspace_id: workspace_id.clone(),
                item_id: item_id.clone(),
                source_table_name: table,
                source_schema: source_schema.clone(),
            }),
            Self::Kusto {
                workspace_id,
                item_id,
                cluster_uri,
                database_name,
            } => SourceTableProperties::Kusto(KustoTableProperties {
                source_type: "KustoTable",
                workspace_id: workspace_id.clone(),
                item_id: item_id.clone(),
                cluster_uri: cluster_uri.clone(),
                database_name: database_name.clone(),
                source_table_name: table,
            }),
        }
    }

    /// `LakehouseTable` properties for a relationship `dataBindingTable`
    /// (Fabric only permits `LakehouseTable` there).
    fn lakehouse_table(&self, table: String) -> Result<LakehouseTableProperties> {
        match self {
            Self::Lakehouse {
                workspace_id,
                item_id,
                source_schema,
            } => Ok(LakehouseTableProperties {
                source_type: "LakehouseTable",
                workspace_id: workspace_id.clone(),
                item_id: item_id.clone(),
                source_table_name: table,
                source_schema: source_schema.clone(),
            }),
            Self::Kusto { .. } => Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Relationship contextualizations require a LakehouseTable source.".to_string(),
                "Fabric only supports LakehouseTable for a relationship dataBindingTable; \
                 remove the KustoTable source override on this relationship.",
            )
            .into()),
        }
    }
}

/// CLI-flag defaults plus the parsed binding map. Per-item sources are resolved
/// lazily so entities/relationships can each pick their own Lakehouse/Eventhouse.
#[derive(Debug)]
struct BindingContext {
    default_workspace_id: Option<String>,
    default_item_id: Option<String>,
    default_schema: Option<String>,
    spec: BindingSpec,
}

impl BindingContext {
    /// Merge a local source override over the global `source` and CLI-flag
    /// defaults into a validated [`ResolvedSource`].
    fn resolve_source(&self, local: Option<&SourceSpec>) -> Result<ResolvedSource> {
        let pick = |f: &dyn Fn(&SourceSpec) -> Option<String>| -> Option<String> {
            local
                .and_then(f)
                .or_else(|| self.spec.source.as_ref().and_then(f))
        };

        let source_type =
            pick(&|s| s.source_type.clone()).unwrap_or_else(|| "LakehouseTable".to_string());
        let workspace_id = pick(&|s| s.workspace_id.clone())
            .or_else(|| self.default_workspace_id.clone())
            .ok_or_else(|| {
                missing_source_field(
                    "workspace ID",
                    "Pass --lakehouse-workspace <WS> (or --workspace), or set source.workspaceId.",
                )
            })?;
        let item_id = pick(&|s| s.item_id.clone())
            .or_else(|| self.default_item_id.clone())
            .ok_or_else(|| {
                missing_source_field(
                    "data source item ID",
                    "Pass --lakehouse <ITEM_ID>, or set source.itemId in the binding map.",
                )
            })?;

        match source_type.as_str() {
            "LakehouseTable" => {
                let source_schema = pick(&|s| s.source_schema.clone())
                    .or_else(|| self.default_schema.clone())
                    .or_else(|| Some("dbo".to_string()));
                Ok(ResolvedSource::Lakehouse {
                    workspace_id,
                    item_id,
                    source_schema,
                })
            }
            "KustoTable" => {
                let cluster_uri = pick(&|s| s.cluster_uri.clone()).ok_or_else(|| {
                    missing_source_field("clusterUri", "Set source.clusterUri for a KustoTable.")
                })?;
                let database_name = pick(&|s| s.database_name.clone()).ok_or_else(|| {
                    missing_source_field(
                        "databaseName",
                        "Set source.databaseName for a KustoTable.",
                    )
                })?;
                Ok(ResolvedSource::Kusto {
                    workspace_id,
                    item_id,
                    cluster_uri,
                    database_name,
                })
            }
            other => Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown data source type '{other}'."),
                "Supported source types: LakehouseTable, KustoTable.",
            )
            .into()),
        }
    }
}

fn missing_source_field(what: &str, hint: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::InvalidInput,
        format!("A {what} is required to generate data bindings."),
        hint.to_string(),
    )
    .into()
}

/// Resolve an optional [`BindingContext`] from CLI flags and/or a binding-map
/// file. Binding generation is enabled when either `--lakehouse` or `--bindings`
/// is supplied. Coordinates resolve per source: override → map `source` → flags.
fn resolve_binding_context(
    workspace: Option<&str>,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
    schema: Option<&str>,
    bindings: Option<&str>,
) -> Result<Option<BindingContext>> {
    if lakehouse.is_none() && bindings.is_none() {
        return Ok(None);
    }

    let spec: BindingSpec = if let Some(path) = bindings {
        let raw = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read binding map '{path}': {e}"))?;
        serde_json::from_str(&raw).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid binding map JSON in '{path}': {e}"),
                "Expected {\"entities\":{...},\"relationships\":{...}}. \
                 See: fabio context examples ontology",
            )
        })?
    } else {
        BindingSpec::default()
    };

    Ok(Some(BindingContext {
        default_workspace_id: lakehouse_workspace.or(workspace).map(str::to_string),
        default_item_id: lakehouse.map(str::to_string),
        default_schema: schema.map(str::to_string),
        spec,
    }))
}

// ─── Order-safe binding serialization ────────────────────────────────────────
//
// Production builds do NOT enable serde_json's `preserve_order` feature, so
// `serde_json::Value` maps serialize keys alphabetically. The Fabric Ontology
// API deserializes `sourceTableProperties` with an ordered, discriminator-first
// reader — `sourceType` MUST be the first key. Struct field order is always
// preserved by serde regardless of the map feature, and an untagged enum
// serializes as its inner struct, so modelling the payloads as structs
// guarantees correct ordering on every build (never round-trip through Value).

#[derive(serde::Serialize)]
#[serde(untagged)]
enum SourceTableProperties {
    Lakehouse(LakehouseTableProperties),
    Kusto(KustoTableProperties),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LakehouseTableProperties {
    source_type: &'static str,
    workspace_id: String,
    item_id: String,
    source_table_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_schema: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KustoTableProperties {
    source_type: &'static str,
    workspace_id: String,
    item_id: String,
    cluster_uri: String,
    database_name: String,
    source_table_name: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PropertyBinding {
    source_column_name: String,
    target_property_id: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DataBindingConfiguration {
    data_binding_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp_column_name: Option<String>,
    property_bindings: Vec<PropertyBinding>,
    source_table_properties: SourceTableProperties,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DataBinding {
    id: String,
    data_binding_configuration: DataBindingConfiguration,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyRefBinding {
    source_column_name: String,
    target_property_id: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Contextualization {
    id: String,
    data_binding_table: LakehouseTableProperties,
    source_key_ref_bindings: Vec<KeyRefBinding>,
    target_key_ref_bindings: Vec<KeyRefBinding>,
}

/// Look up an entity binding by exact class label, then by URI local name.
fn entity_binding<'a>(spec: &'a BindingSpec, class: &OwlClass) -> Option<&'a EntityBindingSpec> {
    spec.entities
        .get(&class.label)
        .or_else(|| spec.entities.get(&uri_local_name(&class.uri)))
}

/// Sanitize a label into a Fabric type/property name matching the schema
/// pattern `^[a-zA-Z][a-zA-Z0-9_-]{0,127}$` (e.g. "has site" → "`has_site`").
fn sanitize_name(label: &str) -> String {
    let mut name = String::new();
    let mut prev_us = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            name.push(ch);
            prev_us = false;
        } else if !prev_us {
            name.push('_');
            prev_us = true;
        }
    }
    let trimmed = name.trim_matches(|c| c == '_' || c == '-').to_string();
    let mut name = if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
    {
        trimmed
    } else {
        format!("type_{trimmed}")
    };
    name.truncate(128);
    name
}

// ─── Fabric Format Generator ─────────────────────────────────────────────────

#[derive(Debug)]
struct FabricPart {
    path: String,
    content: String,
}

#[allow(clippy::too_many_lines)]
fn generate_fabric_parts(
    model: &OwlModel,
    binding: Option<&BindingContext>,
) -> Result<Vec<FabricPart>> {
    let mut parts = Vec::new();

    // Root definition.json
    parts.push(FabricPart {
        path: "definition.json".to_string(),
        content: "{}".to_string(),
    });

    // Build class URI → ID mapping
    let mut class_ids: HashMap<String, String> = HashMap::new();
    for (i, class) in model.classes.iter().enumerate() {
        let id = format!("888{:010}", i + 1);
        class_ids.insert(class.uri.clone(), id);
    }

    // Per-class identifier property ids (full entityIdParts) for contextualizations.
    let mut identifier_ids: HashMap<String, Vec<String>> = HashMap::new();

    // Generate EntityTypes
    for class in &model.classes {
        let type_id = class_ids.get(&class.uri).unwrap();

        // Collect properties for this class
        let mut properties: Vec<Value> = Vec::new();
        let mut id_parts: Vec<String> = Vec::new();
        let mut display_name_id: Option<String> = None;
        // Ordered (source column, property id) pairs for the data binding.
        let mut column_bindings: Vec<(String, String)> = Vec::new();

        let entity_spec = binding.and_then(|b| entity_binding(&b.spec, class));

        for (pi, prop) in model
            .datatype_properties
            .iter()
            .filter(|p| p.domain_uri == class.uri)
            .enumerate()
        {
            let prop_id = format!("{type_id}{:02}", pi + 1);
            properties.push(serde_json::json!({
                "id": prop_id,
                "name": sanitize_name(&prop.label),
                "valueType": prop.property_type,
            }));

            // Source column defaults to the property label; overridable per binding map.
            let source_column = entity_spec
                .and_then(|e| e.columns.get(&prop.label).cloned())
                .unwrap_or_else(|| prop.label.clone());
            column_bindings.push((source_column, prop_id.clone()));

            if prop.is_identifier {
                id_parts.push(prop_id.clone());
            }
            // Use first string property as display name if no identifier found
            if display_name_id.is_none() && prop.property_type == "String" {
                display_name_id = Some(prop_id.clone());
            }
        }

        // If no identifier was marked, use first property
        if id_parts.is_empty()
            && let Some(first) = properties.first()
            && let Some(pid) = first.get("id").and_then(Value::as_str)
        {
            id_parts.push(pid.to_string());
        }

        identifier_ids.insert(class.uri.clone(), id_parts.clone());

        let entity_def = serde_json::json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/ontology/entityType/1.0.0/schema.json",
            "id": type_id,
            "namespace": "usertypes",
            "name": sanitize_name(&class.label),
            "namespaceType": "Custom",
            "visibility": "Visible",
            "displayNamePropertyId": display_name_id.as_deref().unwrap_or(""),
            "entityIdParts": id_parts,
            "properties": properties,
        });

        parts.push(FabricPart {
            path: format!("EntityTypes/{type_id}/definition.json"),
            content: serde_json::to_string_pretty(&entity_def).unwrap_or_default(),
        });

        // Emit a DataBinding when a data source is configured.
        if let Some(ctx) = binding {
            parts.push(build_entity_data_binding(
                ctx,
                &class.label,
                type_id,
                &class.label.to_lowercase(),
                column_bindings,
                entity_spec,
            )?);
        }
    }

    // Generate RelationshipTypes
    for (i, rel) in model.object_properties.iter().enumerate() {
        let rel_id = format!("999{:010}", i + 1);

        let source_id = class_ids.get(&rel.domain_uri).cloned().unwrap_or_default();
        let target_id = class_ids.get(&rel.range_uri).cloned().unwrap_or_default();

        if source_id.is_empty() || target_id.is_empty() {
            continue; // Skip if source or target class not found
        }

        let rel_def = serde_json::json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/ontology/relationshipType/1.0.0/schema.json",
            "id": rel_id,
            "namespace": "usertypes",
            "name": sanitize_name(&rel.label),
            "namespaceType": "Custom",
            "source": {"entityTypeId": source_id},
            "target": {"entityTypeId": target_id},
        });

        parts.push(FabricPart {
            path: format!("RelationshipTypes/{rel_id}/definition.json"),
            content: serde_json::to_string_pretty(&rel_def).unwrap_or_default(),
        });

        // Emit a Contextualization when the binding map describes this relationship.
        // Relationship key columns cannot be inferred from OWL, so we only bind
        // relationships explicitly listed in --bindings.
        if let Some(ctx) = binding {
            let Some(rel_spec) = ctx.spec.relationships.get(&rel.label) else {
                if !ctx.spec.relationships.is_empty() {
                    eprintln!(
                        "[ontology import] No binding map entry for relationship '{}'; \
                         emitting type without a contextualization.",
                        rel.label
                    );
                }
                continue;
            };
            let (Some(source_parts), Some(target_parts)) = (
                identifier_ids.get(&rel.domain_uri),
                identifier_ids.get(&rel.range_uri),
            ) else {
                continue;
            };

            parts.push(build_relationship_contextualization(
                ctx,
                &rel.label,
                &rel_id,
                rel_spec,
                source_parts,
                target_parts,
            )?);
        }
    }

    Ok(parts)
}

/// Build an `EntityType` `DataBinding` part for one entity, applying binding-map
/// overrides and validating the dataBindingType/source combination against the
/// Fabric schema rules. Shared by `import` (generated ids) and `bind` (live ids).
fn build_entity_data_binding(
    ctx: &BindingContext,
    entity_name: &str,
    entity_id: &str,
    default_table: &str,
    column_bindings: Vec<(String, String)>,
    entity_spec: Option<&EntityBindingSpec>,
) -> Result<FabricPart> {
    let table = entity_spec
        .and_then(|e| e.table.clone())
        .unwrap_or_else(|| default_table.to_string());

    let data_binding_type = entity_spec
        .and_then(|e| e.data_binding_type.clone())
        .unwrap_or_else(|| "NonTimeSeries".to_string());
    if data_binding_type != "NonTimeSeries" && data_binding_type != "TimeSeries" {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Entity '{entity_name}' has invalid dataBindingType '{data_binding_type}'."),
            "dataBindingType must be 'NonTimeSeries' or 'TimeSeries'.",
        )
        .into());
    }
    let timestamp_column_name = entity_spec.and_then(|e| e.timestamp_column.clone());
    if data_binding_type == "TimeSeries" && timestamp_column_name.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Entity '{entity_name}' is TimeSeries but has no timestampColumn."),
            "Set entities.<name>.timestampColumn for TimeSeries data bindings.",
        )
        .into());
    }

    let source = ctx.resolve_source(entity_spec.and_then(|e| e.source.as_ref()))?;
    if !source.is_lakehouse() && data_binding_type == "NonTimeSeries" {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Entity '{entity_name}' uses a KustoTable source with NonTimeSeries."),
            "Fabric permits KustoTable only with TimeSeries; set dataBindingType to \
             'TimeSeries' or use a LakehouseTable source.",
        )
        .into());
    }

    let binding_uuid = deterministic_uuid("binding", entity_name);
    let data_binding = DataBinding {
        id: binding_uuid.clone(),
        data_binding_configuration: DataBindingConfiguration {
            data_binding_type,
            timestamp_column_name,
            property_bindings: column_bindings
                .into_iter()
                .map(|(col, pid)| PropertyBinding {
                    source_column_name: col,
                    target_property_id: pid,
                })
                .collect(),
            source_table_properties: source.table_properties(table),
        },
    };
    Ok(FabricPart {
        path: format!("EntityTypes/{entity_id}/DataBindings/{binding_uuid}.json"),
        content: serde_json::to_string_pretty(&data_binding).unwrap_or_default(),
    })
}

/// Build a `RelationshipType` `Contextualization` part, zipping key columns to the
/// endpoints' `entityIdParts`. Shared by `import` and `bind`.
fn build_relationship_contextualization(
    ctx: &BindingContext,
    relationship_name: &str,
    relationship_id: &str,
    rel_spec: &RelationshipBindingSpec,
    source_id_parts: &[String],
    target_id_parts: &[String],
) -> Result<FabricPart> {
    let source_refs = zip_key_refs(
        relationship_name,
        "source",
        &rel_spec.source_columns,
        source_id_parts,
    )?;
    let target_refs = zip_key_refs(
        relationship_name,
        "target",
        &rel_spec.target_columns,
        target_id_parts,
    )?;

    let source = ctx.resolve_source(rel_spec.source.as_ref())?;
    let data_binding_table = source.lakehouse_table(rel_spec.table.clone())?;

    let ctx_uuid = deterministic_uuid("contextualization", relationship_name);
    let contextualization = Contextualization {
        id: ctx_uuid.clone(),
        data_binding_table,
        source_key_ref_bindings: source_refs,
        target_key_ref_bindings: target_refs,
    };
    Ok(FabricPart {
        path: format!("RelationshipTypes/{relationship_id}/Contextualizations/{ctx_uuid}.json"),
        content: serde_json::to_string_pretty(&contextualization).unwrap_or_default(),
    })
}

/// Zip relationship key columns against an entity's `entityIdParts`, producing
/// one [`KeyRefBinding`] per identifier part. Arities must match: a composite
/// identifier needs one column per part.
fn zip_key_refs(
    relationship: &str,
    end: &str,
    columns: &[String],
    id_parts: &[String],
) -> Result<Vec<KeyRefBinding>> {
    if columns.len() != id_parts.len() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Relationship '{relationship}' {end} has {} key column(s) but the {end} entity \
                 has {} identifier part(s).",
                columns.len(),
                id_parts.len()
            ),
            format!(
                "Provide exactly {} {end}Column(s), one per entityIdParts entry, in the binding map.",
                id_parts.len()
            ),
        )
        .into());
    }
    Ok(columns
        .iter()
        .zip(id_parts.iter())
        .map(|(col, pid)| KeyRefBinding {
            source_column_name: col.clone(),
            target_property_id: pid.clone(),
        })
        .collect())
}

/// Deterministic UUID v5 keyed by (kind, name) so re-imports keep stable
/// binding IDs. Fabric silently drops non-UUID data-binding IDs.
fn deterministic_uuid(kind: &str, name: &str) -> String {
    uuid::Uuid::new_v5(&BINDING_NAMESPACE, format!("{kind}:{name}").as_bytes()).to_string()
}

// ─── Bind an existing ontology ───────────────────────────────────────────────

/// An entity type parsed from a live ontology definition.
#[derive(Debug)]
struct LiveEntity {
    id: String,
    name: String,
    id_parts: Vec<String>,
    /// (property id, property name), in definition order.
    properties: Vec<(String, String)>,
}

/// A relationship type parsed from a live ontology definition.
#[derive(Debug)]
struct LiveRelationship {
    id: String,
    name: String,
    source_type_id: String,
    target_type_id: String,
}

/// Bind an existing ontology's types to data sources without re-importing OWL.
/// Fetches the current definition, matches entity/relationship types by name,
/// generates DataBindings/Contextualizations against the live type/property ids,
/// merges them into the definition, and pushes via `updateDefinition`.
#[allow(clippy::too_many_arguments)]
pub async fn bind_ontology(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
    lakehouse_schema: Option<&str>,
    bindings: Option<&str>,
) -> Result<()> {
    let ctx = resolve_binding_context(
        Some(workspace),
        lakehouse,
        lakehouse_workspace,
        lakehouse_schema,
        bindings,
    )?
    .ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Nothing to bind: no data source provided.".to_string(),
            "Pass --lakehouse <ITEM_ID> and/or --bindings <map.json>.",
        )
    })?;

    // Fetch the current definition.
    let data = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology bind", "Contributor"))?;

    let existing = data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("No definition parts returned for ontology '{id}'"))?;

    let (entities, relationships) = parse_live_types(existing);

    let (binding_parts, bound_entities, bound_rels) =
        generate_bindings_for_live(&entities, &relationships, &ctx)?;

    if output::dry_run_guard(
        cli,
        "ontology bind",
        &serde_json::json!({
            "id": id,
            "entity_types": entities.len(),
            "relationship_types": relationships.len(),
            "entity_bindings": bound_entities.len(),
            "contextualizations": bound_rels.len(),
        }),
    ) {
        return Ok(());
    }

    let merged = merge_definition_parts(existing, binding_parts, &bound_entities, &bound_rels);
    let body = serde_json::json!({ "definition": { "parts": merged } });

    let resp = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology bind", "Contributor"))?;

    if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "status": "bound",
            "id": id,
            "entity_bindings": bound_entities.len(),
            "contextualizations": bound_rels.len(),
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &resp, "id");
    }
    Ok(())
}

/// Decode a base64 definition part payload into JSON.
fn decode_part(part: &Value) -> Value {
    part.get("payload")
        .and_then(Value::as_str)
        .and_then(|p| BASE64.decode(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

/// Parse EntityTypes/RelationshipTypes (the `.../<id>/definition.json` parts)
/// from a fetched ontology definition.
fn parse_live_types(parts: &[Value]) -> (Vec<LiveEntity>, Vec<LiveRelationship>) {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();

    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        let segs: Vec<&str> = path.split('/').collect();
        if segs.len() != 3 || segs[2] != "definition.json" {
            continue;
        }
        let v = decode_part(part);
        match segs[0] {
            "EntityTypes" => {
                let name = v.get("name").and_then(Value::as_str).unwrap_or("");
                let eid = v.get("id").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() || eid.is_empty() {
                    continue;
                }
                let id_parts = v
                    .get("entityIdParts")
                    .and_then(Value::as_array)
                    .map_or_else(Vec::new, |a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    });
                let properties =
                    v.get("properties")
                        .and_then(Value::as_array)
                        .map_or_else(Vec::new, |a| {
                            a.iter()
                                .filter_map(|p| {
                                    let pid = p.get("id").and_then(Value::as_str)?;
                                    let pname = p.get("name").and_then(Value::as_str)?;
                                    Some((pid.to_string(), pname.to_string()))
                                })
                                .collect()
                        });
                entities.push(LiveEntity {
                    id: eid.to_string(),
                    name: name.to_string(),
                    id_parts,
                    properties,
                });
            }
            "RelationshipTypes" => {
                let name = v.get("name").and_then(Value::as_str).unwrap_or("");
                let rid = v.get("id").and_then(Value::as_str).unwrap_or("");
                let source_type_id = v
                    .get("source")
                    .and_then(|s| s.get("entityTypeId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let target_type_id = v
                    .get("target")
                    .and_then(|t| t.get("entityTypeId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name.is_empty() || rid.is_empty() {
                    continue;
                }
                relationships.push(LiveRelationship {
                    id: rid.to_string(),
                    name: name.to_string(),
                    source_type_id: source_type_id.to_string(),
                    target_type_id: target_type_id.to_string(),
                });
            }
            _ => {}
        }
    }
    (entities, relationships)
}

/// Match a binding-map key against a live (already-sanitized) type name, either
/// exactly or after sanitization (so users can key the map with OWL labels).
fn name_matches(key: &str, live_name: &str) -> bool {
    key == live_name || sanitize_name(key) == live_name
}

/// Generate binding parts against live types. Entities are bound by convention
/// when the map has no `entities` block, else only the entities named in the
/// map. Relationships are bound only when named in the map. Returns the parts
/// plus the sets of entity/relationship ids that were (re)bound. Map keys that
/// match no live type are a hard error (Fabric addresses types by name).
#[allow(clippy::too_many_lines)]
fn generate_bindings_for_live(
    entities: &[LiveEntity],
    relationships: &[LiveRelationship],
    ctx: &BindingContext,
) -> Result<(Vec<FabricPart>, HashSet<String>, HashSet<String>)> {
    let mut parts = Vec::new();
    let mut bound_entities = HashSet::new();
    let mut bound_rels = HashSet::new();
    let mut used_entity_keys: HashSet<String> = HashSet::new();
    let mut used_rel_keys: HashSet<String> = HashSet::new();

    let by_id: HashMap<&str, &LiveEntity> = entities.iter().map(|e| (e.id.as_str(), e)).collect();

    // Entities: bind all when no `entities` block is given; else only listed ones.
    let select_all = ctx.spec.entities.is_empty();
    for entity in entities {
        let matched = ctx
            .spec
            .entities
            .iter()
            .find(|(k, _)| name_matches(k, &entity.name));
        if let Some((k, _)) = matched {
            used_entity_keys.insert((*k).clone());
        }
        let entity_spec = matched.map(|(_, v)| v);
        if !select_all && entity_spec.is_none() {
            continue;
        }

        let column_bindings: Vec<(String, String)> = entity
            .properties
            .iter()
            .map(|(pid, pname)| {
                let col = entity_spec
                    .and_then(|s| {
                        s.columns
                            .iter()
                            .find(|(k, _)| name_matches(k, pname))
                            .map(|(_, v)| v.clone())
                    })
                    .unwrap_or_else(|| pname.clone());
                (col, pid.clone())
            })
            .collect();

        parts.push(build_entity_data_binding(
            ctx,
            &entity.name,
            &entity.id,
            &entity.name.to_lowercase(),
            column_bindings,
            entity_spec,
        )?);
        bound_entities.insert(entity.id.clone());
    }

    // Relationships: only those named in the map.
    for rel in relationships {
        let Some((k, rel_spec)) = ctx
            .spec
            .relationships
            .iter()
            .find(|(k, _)| name_matches(k, &rel.name))
        else {
            continue;
        };
        used_rel_keys.insert(k.clone());

        let (Some(src), Some(tgt)) = (
            by_id.get(rel.source_type_id.as_str()),
            by_id.get(rel.target_type_id.as_str()),
        ) else {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "Relationship '{}' references an entity type not present in the definition.",
                    rel.name
                ),
                "Fetch the definition to inspect: fabio ontology get-definition --decode.",
            )
            .into());
        };

        parts.push(build_relationship_contextualization(
            ctx,
            &rel.name,
            &rel.id,
            rel_spec,
            &src.id_parts,
            &tgt.id_parts,
        )?);
        bound_rels.insert(rel.id.clone());
    }

    // Strict: every map key must match a live type name.
    let unmatched_entities: Vec<&String> = ctx
        .spec
        .entities
        .keys()
        .filter(|k| !used_entity_keys.contains(*k))
        .collect();
    let unmatched_rels: Vec<&String> = ctx
        .spec
        .relationships
        .keys()
        .filter(|k| !used_rel_keys.contains(*k))
        .collect();
    if !unmatched_entities.is_empty() || !unmatched_rels.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Binding map names types that do not exist in the ontology: \
                 entities={unmatched_entities:?}, relationships={unmatched_rels:?}."
            ),
            "Names must match the ontology's entity/relationship type names \
             (see: fabio ontology get-definition --decode). Renaming a type after \
             data is mapped is unsupported in Fabric.",
        )
        .into());
    }

    Ok((parts, bound_entities, bound_rels))
}

/// Merge freshly generated binding parts into the existing definition parts,
/// dropping prior DataBindings/Contextualizations for the (re)bound types so the
/// operation is idempotent, and leaving all other parts untouched.
fn merge_definition_parts(
    existing: &[Value],
    new_parts: Vec<FabricPart>,
    bound_entities: &HashSet<String>,
    bound_rels: &HashSet<String>,
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for part in existing {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        let segs: Vec<&str> = path.split('/').collect();
        let drop = segs.len() >= 3
            && ((segs[0] == "EntityTypes"
                && segs[2] == "DataBindings"
                && bound_entities.contains(segs[1]))
                || (segs[0] == "RelationshipTypes"
                    && segs[2] == "Contextualizations"
                    && bound_rels.contains(segs[1])));
        if !drop {
            out.push(part.clone());
        }
    }
    for p in new_parts {
        out.push(serde_json::json!({
            "path": p.path,
            "payload": BASE64.encode(p.content.as_bytes()),
            "payloadType": "InlineBase64",
        }));
    }
    out
}

// ─── Directory Export ────────────────────────────────────────────────────────

fn write_to_directory(dir: &str, model: &OwlModel, parts: &[FabricPart]) -> Result<()> {
    for part in parts {
        let full_path = Path::new(dir).join(&part.path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full_path, &part.content)?;
    }

    eprintln!(
        "[ontology import] Exported {} entity types, {} relationship types to {dir}",
        model.classes.len(),
        model.object_properties.len()
    );
    Ok(())
}

// ─── Fabric API Push ─────────────────────────────────────────────────────────

async fn push_to_fabric(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parts: &[FabricPart],
) -> Result<()> {
    // Build definition parts array
    let api_parts: Vec<Value> = parts
        .iter()
        .map(|p| {
            serde_json::json!({
                "path": p.path,
                "payload": BASE64.encode(p.content.as_bytes()),
                "payloadType": "InlineBase64"
            })
        })
        .collect();

    let body = serde_json::json!({
        "definition": {
            "parts": api_parts
        }
    });

    let data = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology import", "Contributor"))?;

    let entity_count = parts
        .iter()
        .filter(|p| p.path.contains("EntityTypes"))
        .count();
    let rel_count = parts
        .iter()
        .filter(|p| p.path.contains("RelationshipTypes"))
        .count();

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "status": "imported",
            "id": id,
            "entity_types": entity_count,
            "relationship_types": rel_count,
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rdf_xml_classes() {
        let rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#">
  <owl:Class rdf:about="http://example.org/Customer">
    <rdfs:label>Customer</rdfs:label>
  </owl:Class>
  <owl:Class rdf:about="http://example.org/Order">
    <rdfs:label>Order</rdfs:label>
  </owl:Class>
</rdf:RDF>"#;
        let model = parse_rdf_xml(rdf);
        assert_eq!(model.classes.len(), 2);
        assert_eq!(model.classes[0].label, "Customer");
        assert_eq!(model.classes[1].label, "Order");
    }

    #[test]
    fn test_parse_rdf_xml_properties_with_types() {
        let rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:xsd="http://www.w3.org/2001/XMLSchema#"
         xmlns:ont="http://example.org/">
  <owl:Class rdf:about="http://example.org/Product">
    <rdfs:label>Product</rdfs:label>
  </owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/product_price">
    <rdfs:label>price</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Product"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#decimal"/>
    <ont:propertyType>decimal</ont:propertyType>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/product_id">
    <rdfs:label>productId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Product"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
</rdf:RDF>"#;
        let model = parse_rdf_xml(rdf);
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.datatype_properties.len(), 2);

        let price = &model.datatype_properties[0];
        assert_eq!(price.label, "price");
        assert_eq!(price.property_type, "Double");
        assert!(!price.is_identifier);

        let pid = &model.datatype_properties[1];
        assert_eq!(pid.label, "productId");
        assert!(pid.is_identifier);
    }

    #[test]
    fn test_parse_rdf_xml_relationships() {
        let rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#">
  <owl:Class rdf:about="http://example.org/Customer"><rdfs:label>Customer</rdfs:label></owl:Class>
  <owl:Class rdf:about="http://example.org/Order"><rdfs:label>Order</rdfs:label></owl:Class>
  <owl:ObjectProperty rdf:about="http://example.org/places">
    <rdfs:label>places</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Customer"/>
    <rdfs:range rdf:resource="http://example.org/Order"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;
        let model = parse_rdf_xml(rdf);
        assert_eq!(model.object_properties.len(), 1);
        assert_eq!(model.object_properties[0].label, "places");
        assert_eq!(
            model.object_properties[0].domain_uri,
            "http://example.org/Customer"
        );
        assert_eq!(
            model.object_properties[0].range_uri,
            "http://example.org/Order"
        );
    }

    #[test]
    fn test_parse_json_ld_owl_classes() {
        let jsonld = r#"{
            "@graph": [
                {"@id": "http://ex.org/Cat", "@type": "owl:Class", "rdfs:label": "Category"},
                {"@id": "http://ex.org/Item", "@type": "owl:Class", "rdfs:label": "Item"}
            ]
        }"#;
        let model = parse_json_ld(jsonld).unwrap();
        assert_eq!(model.classes.len(), 2);
        assert_eq!(model.classes[0].label, "Category");
        assert_eq!(model.classes[1].label, "Item");
    }

    #[test]
    fn test_parse_json_ld_fabric_context_output() {
        let jsonld = r#"{"data": {"@context": {}, "@graph": [
            {"@id": "urn:fabric:item:abc", "@type": "fabric:Notebook", "name": "ETL"},
            {"@id": "urn:fabric:item:def", "@type": "fabric:Lakehouse", "name": "Sales"},
            {"@id": "urn:fabric:workspace:ws1", "@type": "fabric:Workspace", "name": "Demo"}
        ]}}"#;
        let model = parse_json_ld(jsonld).unwrap();
        // Workspaces are excluded, unique types extracted
        assert_eq!(model.classes.len(), 2);
        let names: Vec<&str> = model.classes.iter().map(|c| c.label.as_str()).collect();
        assert!(names.contains(&"Notebook"));
        assert!(names.contains(&"Lakehouse"));
    }

    #[test]
    fn test_xsd_type_mapping() {
        assert_eq!(
            xsd_to_fabric_type("http://www.w3.org/2001/XMLSchema#string"),
            "String"
        );
        assert_eq!(
            xsd_to_fabric_type("http://www.w3.org/2001/XMLSchema#integer"),
            "BigInt"
        );
        assert_eq!(
            xsd_to_fabric_type("http://www.w3.org/2001/XMLSchema#decimal"),
            "Double"
        );
        assert_eq!(
            xsd_to_fabric_type("http://www.w3.org/2001/XMLSchema#boolean"),
            "Boolean"
        );
        assert_eq!(
            xsd_to_fabric_type("http://www.w3.org/2001/XMLSchema#dateTime"),
            "DateTime"
        );
        assert_eq!(xsd_to_fabric_type("http://example.org/unknown"), "String");
    }

    #[test]
    fn test_playground_type_mapping() {
        assert_eq!(playground_type_to_fabric("string"), "String");
        assert_eq!(playground_type_to_fabric("enum"), "String");
        assert_eq!(playground_type_to_fabric("integer"), "BigInt");
        assert_eq!(playground_type_to_fabric("decimal"), "Double");
        assert_eq!(playground_type_to_fabric("boolean"), "Boolean");
        assert_eq!(playground_type_to_fabric("datetime"), "DateTime");
        assert_eq!(playground_type_to_fabric("date"), "DateTime");
    }

    #[test]
    fn test_generate_fabric_parts() {
        let model = OwlModel {
            classes: vec![
                OwlClass {
                    uri: "http://ex.org/A".to_string(),
                    label: "TypeA".to_string(),
                },
                OwlClass {
                    uri: "http://ex.org/B".to_string(),
                    label: "TypeB".to_string(),
                },
            ],
            datatype_properties: vec![OwlDatatypeProperty {
                label: "name".to_string(),
                domain_uri: "http://ex.org/A".to_string(),
                property_type: "String".to_string(),
                is_identifier: true,
            }],
            object_properties: vec![OwlObjectProperty {
                label: "relatesTo".to_string(),
                domain_uri: "http://ex.org/A".to_string(),
                range_uri: "http://ex.org/B".to_string(),
            }],
        };
        let parts = generate_fabric_parts(&model, None).unwrap();
        // root + 2 entities + 1 relationship = 4 parts
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0].path, "definition.json");
        assert!(parts[1].path.contains("EntityTypes"));
        assert!(parts[2].path.contains("EntityTypes"));
        assert!(parts[3].path.contains("RelationshipTypes"));

        // Verify entity content
        let entity: serde_json::Value = serde_json::from_str(&parts[1].content).unwrap();
        assert_eq!(entity["name"], "TypeA");
        assert_eq!(entity["properties"][0]["name"], "name");
        assert_eq!(entity["properties"][0]["valueType"], "String");
    }

    #[test]
    fn test_uri_local_name() {
        assert_eq!(uri_local_name("http://example.org/foo/Bar"), "Bar");
        assert_eq!(uri_local_name("http://example.org#Baz"), "Baz");
        assert_eq!(uri_local_name("JustAName"), "JustAName");
    }

    #[test]
    fn test_content_detection_xml() {
        let xml_content = "<?xml version=\"1.0\"?>\n<rdf:RDF>...</rdf:RDF>";
        assert!(xml_content.trim_start().starts_with('<'));
    }

    #[test]
    fn test_content_detection_json() {
        let json_content = "{\"@graph\": []}";
        let trimmed = json_content.trim_start();
        assert!(trimmed.starts_with('{') || trimmed.starts_with('['));
    }

    #[test]
    fn test_serialize_to_rdf_xml() {
        let model = OwlModel {
            classes: vec![OwlClass {
                uri: "http://ex.org/Thing".to_string(),
                label: "Thing".to_string(),
            }],
            datatype_properties: vec![OwlDatatypeProperty {
                label: "name".to_string(),
                domain_uri: "http://ex.org/Thing".to_string(),
                property_type: "String".to_string(),
                is_identifier: true,
            }],
            object_properties: vec![],
        };
        let rdf = serialize_to_rdf_xml(&model);
        assert!(rdf.contains("owl:Class"));
        assert!(rdf.contains("Thing"));
        assert!(rdf.contains("owl:DatatypeProperty"));
        assert!(rdf.contains("ont:isIdentifier"));
        assert!(rdf.contains("XMLSchema#string"));
    }

    #[test]
    fn test_serialize_to_jsonld() {
        let model = OwlModel {
            classes: vec![
                OwlClass {
                    uri: "http://ex.org/A".to_string(),
                    label: "A".to_string(),
                },
                OwlClass {
                    uri: "http://ex.org/B".to_string(),
                    label: "B".to_string(),
                },
            ],
            datatype_properties: vec![OwlDatatypeProperty {
                label: "score".to_string(),
                domain_uri: "http://ex.org/A".to_string(),
                property_type: "Double".to_string(),
                is_identifier: false,
            }],
            object_properties: vec![OwlObjectProperty {
                label: "links".to_string(),
                domain_uri: "http://ex.org/A".to_string(),
                range_uri: "http://ex.org/B".to_string(),
            }],
        };
        let jsonld = serialize_to_jsonld(&model);
        let doc: serde_json::Value = serde_json::from_str(&jsonld).unwrap();
        assert!(doc.get("@context").is_some());
        let graph = doc["@graph"].as_array().unwrap();
        // 2 classes + 1 property + 1 relationship = 4 nodes
        assert_eq!(graph.len(), 4);
        assert_eq!(
            graph.iter().filter(|n| n["@type"] == "owl:Class").count(),
            2
        );
        let rels: Vec<_> = graph
            .iter()
            .filter(|n| n["@type"] == "owl:ObjectProperty")
            .collect();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0]["rdfs:label"], "links");
    }

    #[test]
    fn test_fabric_type_to_xsd() {
        assert_eq!(fabric_type_to_xsd("String"), "string");
        assert_eq!(fabric_type_to_xsd("BigInt"), "integer");
        assert_eq!(fabric_type_to_xsd("Double"), "decimal");
        assert_eq!(fabric_type_to_xsd("Boolean"), "boolean");
        assert_eq!(fabric_type_to_xsd("DateTime"), "dateTime");
        assert_eq!(fabric_type_to_xsd("Unknown"), "string");
    }

    // -----------------------------------------------------------------------
    // Lakehouse / Eventhouse binding generation (validated against the
    // official Fabric Ontology JSON schemas under tests/fixtures/).
    // -----------------------------------------------------------------------

    const WS: &str = "11111111-1111-4111-8111-111111111111";
    const LH: &str = "22222222-2222-4222-8222-222222222222";

    fn lakehouse_ctx(spec: BindingSpec) -> BindingContext {
        BindingContext {
            default_workspace_id: Some(WS.to_string()),
            default_item_id: Some(LH.to_string()),
            default_schema: None,
            spec,
        }
    }

    /// Study(studyId id, studyName) --hasSite--> Site(siteId id).
    fn binding_model() -> OwlModel {
        OwlModel {
            classes: vec![
                OwlClass {
                    uri: "http://ex.org/Study".into(),
                    label: "Study".into(),
                },
                OwlClass {
                    uri: "http://ex.org/Site".into(),
                    label: "Site".into(),
                },
            ],
            datatype_properties: vec![
                OwlDatatypeProperty {
                    label: "studyId".into(),
                    domain_uri: "http://ex.org/Study".into(),
                    property_type: "String".into(),
                    is_identifier: true,
                },
                OwlDatatypeProperty {
                    label: "studyName".into(),
                    domain_uri: "http://ex.org/Study".into(),
                    property_type: "String".into(),
                    is_identifier: false,
                },
                OwlDatatypeProperty {
                    label: "siteId".into(),
                    domain_uri: "http://ex.org/Site".into(),
                    property_type: "String".into(),
                    is_identifier: true,
                },
            ],
            object_properties: vec![OwlObjectProperty {
                label: "has site".into(),
                domain_uri: "http://ex.org/Study".into(),
                range_uri: "http://ex.org/Site".into(),
            }],
        }
    }

    fn find_part<'a>(parts: &'a [FabricPart], needle: &str) -> &'a FabricPart {
        parts
            .iter()
            .find(|p| p.path.contains(needle))
            .unwrap_or_else(|| {
                let all: Vec<&str> = parts.iter().map(|p| p.path.as_str()).collect();
                panic!("no part containing '{needle}'; have: {all:?}")
            })
    }

    fn payload(part: &FabricPart) -> Value {
        serde_json::from_str(&part.content).expect("part content is JSON")
    }

    /// Validate an instance against a vendored official schema.
    fn assert_schema_valid(schema_name: &str, instance: &Value) {
        let path = format!(
            "{}/tests/fixtures/ontology-schemas/{schema_name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let validator = jsonschema::validator_for(&schema).expect("schema compiles");
        let errors: Vec<String> = validator
            .iter_errors(instance)
            .map(|e| e.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "{schema_name} validation failed: {errors:#?}\ninstance:\n{}",
            serde_json::to_string_pretty(instance).unwrap()
        );
    }

    #[test]
    fn generated_types_and_bindings_match_official_schemas() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"has site":{"table":"site","sourceColumns":["study_id"],"targetColumns":["siteId"]}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();

        assert_schema_valid(
            "entityType",
            &payload(find_part(
                &parts,
                "EntityTypes/8880000000001/definition.json",
            )),
        );
        assert_schema_valid(
            "dataBinding",
            &payload(find_part(&parts, "EntityTypes/8880000000001/DataBindings")),
        );
        assert_schema_valid(
            "relationshipType",
            &payload(find_part(
                &parts,
                "RelationshipTypes/9990000000001/definition.json",
            )),
        );
        assert_schema_valid(
            "contextualization",
            &payload(find_part(
                &parts,
                "RelationshipTypes/9990000000001/Contextualizations",
            )),
        );
    }

    #[test]
    fn convention_defaults_lakehouse_table_and_columns() {
        let parts = generate_fabric_parts(
            &binding_model(),
            Some(&lakehouse_ctx(BindingSpec::default())),
        )
        .unwrap();
        let v = payload(find_part(&parts, "EntityTypes/8880000000001/DataBindings"));
        let stp = &v["dataBindingConfiguration"]["sourceTableProperties"];
        assert_eq!(stp["sourceType"], "LakehouseTable");
        assert_eq!(stp["sourceTableName"], "study");
        assert_eq!(stp["sourceSchema"], "dbo");
        assert_eq!(stp["workspaceId"], WS);
        assert_eq!(
            v["dataBindingConfiguration"]["dataBindingType"],
            "NonTimeSeries"
        );
        assert_eq!(
            v["dataBindingConfiguration"]["propertyBindings"][0]["sourceColumnName"],
            "studyId"
        );
    }

    #[test]
    fn binding_map_overrides_table_and_columns() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"entities":{"Study":{"table":"study_dim","source":{"sourceSchema":"silver"},"columns":{"studyId":"study_key"}}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();
        let v = payload(find_part(&parts, "EntityTypes/8880000000001/DataBindings"));
        let stp = &v["dataBindingConfiguration"]["sourceTableProperties"];
        assert_eq!(stp["sourceTableName"], "study_dim");
        assert_eq!(stp["sourceSchema"], "silver");
        assert_eq!(
            v["dataBindingConfiguration"]["propertyBindings"][0]["sourceColumnName"],
            "study_key"
        );
    }

    #[test]
    fn source_type_is_serialized_first() {
        let parts = generate_fabric_parts(
            &binding_model(),
            Some(&lakehouse_ctx(BindingSpec::default())),
        )
        .unwrap();
        let db = find_part(&parts, "EntityTypes/8880000000001/DataBindings");
        let st = db
            .content
            .find("\"sourceType\"")
            .expect("sourceType present");
        for other in ["workspaceId", "itemId", "sourceTableName", "sourceSchema"] {
            let pos = db
                .content
                .find(&format!("\"{other}\""))
                .unwrap_or_else(|| panic!("{other} present"));
            assert!(st < pos, "sourceType must precede {other}");
        }
    }

    #[test]
    fn relationship_name_is_sanitized() {
        let parts = generate_fabric_parts(
            &binding_model(),
            Some(&lakehouse_ctx(BindingSpec::default())),
        )
        .unwrap();
        let v = payload(find_part(
            &parts,
            "RelationshipTypes/9990000000001/definition.json",
        ));
        assert_eq!(v["name"], "has_site");
    }

    #[test]
    fn single_key_contextualization_maps_endpoint_identifiers() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"has site":{"table":"site","sourceColumns":["study_id"],"targetColumns":["siteId"]}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();
        let v = payload(find_part(
            &parts,
            "RelationshipTypes/9990000000001/Contextualizations",
        ));
        assert_eq!(v["dataBindingTable"]["sourceTableName"], "site");
        assert_eq!(v["dataBindingTable"]["sourceType"], "LakehouseTable");
        assert_eq!(v["sourceKeyRefBindings"][0]["sourceColumnName"], "study_id");
        assert_eq!(
            v["sourceKeyRefBindings"][0]["targetPropertyId"],
            "888000000000101"
        );
        assert_eq!(v["targetKeyRefBindings"][0]["sourceColumnName"], "siteId");
        assert_eq!(
            v["targetKeyRefBindings"][0]["targetPropertyId"],
            "888000000000201"
        );
    }

    /// A(a1,a2 composite id) --linked--> B(b1 id).
    fn composite_model() -> OwlModel {
        OwlModel {
            classes: vec![
                OwlClass {
                    uri: "http://ex.org/A".into(),
                    label: "A".into(),
                },
                OwlClass {
                    uri: "http://ex.org/B".into(),
                    label: "B".into(),
                },
            ],
            datatype_properties: vec![
                OwlDatatypeProperty {
                    label: "a1".into(),
                    domain_uri: "http://ex.org/A".into(),
                    property_type: "String".into(),
                    is_identifier: true,
                },
                OwlDatatypeProperty {
                    label: "a2".into(),
                    domain_uri: "http://ex.org/A".into(),
                    property_type: "String".into(),
                    is_identifier: true,
                },
                OwlDatatypeProperty {
                    label: "b1".into(),
                    domain_uri: "http://ex.org/B".into(),
                    property_type: "String".into(),
                    is_identifier: true,
                },
            ],
            object_properties: vec![OwlObjectProperty {
                label: "linked".into(),
                domain_uri: "http://ex.org/A".into(),
                range_uri: "http://ex.org/B".into(),
            }],
        }
    }

    #[test]
    fn composite_key_contextualization() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"linked":{"table":"link","sourceColumns":["a1_fk","a2_fk"],"targetColumns":["b1_fk"]}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&composite_model(), Some(&lakehouse_ctx(spec))).unwrap();
        let v = payload(find_part(&parts, "Contextualizations"));
        assert_schema_valid("contextualization", &v);
        assert_eq!(v["sourceKeyRefBindings"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["sourceKeyRefBindings"][0]["targetPropertyId"],
            "888000000000101"
        );
        assert_eq!(
            v["sourceKeyRefBindings"][1]["targetPropertyId"],
            "888000000000102"
        );
        assert_eq!(v["targetKeyRefBindings"].as_array().unwrap().len(), 1);
        assert_eq!(v["targetKeyRefBindings"][0]["sourceColumnName"], "b1_fk");
    }

    #[test]
    fn composite_key_arity_mismatch_errors() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"linked":{"table":"link","sourceColumns":["only_one"],"targetColumns":["b1_fk"]}}}"#,
        ).unwrap();
        let err =
            generate_fabric_parts(&composite_model(), Some(&lakehouse_ctx(spec))).unwrap_err();
        assert!(err.to_string().contains("key column"), "{err}");
    }

    #[test]
    fn timeseries_lakehouse_binding() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"entities":{"Study":{"dataBindingType":"TimeSeries","timestampColumn":"ts"}}}"#,
        )
        .unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();
        let v = payload(find_part(&parts, "EntityTypes/8880000000001/DataBindings"));
        assert_schema_valid("dataBinding", &v);
        assert_eq!(
            v["dataBindingConfiguration"]["dataBindingType"],
            "TimeSeries"
        );
        assert_eq!(v["dataBindingConfiguration"]["timestampColumnName"], "ts");
    }

    #[test]
    fn timeseries_requires_timestamp() {
        let spec: BindingSpec =
            serde_json::from_str(r#"{"entities":{"Study":{"dataBindingType":"TimeSeries"}}}"#)
                .unwrap();
        let err = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap_err();
        assert!(err.to_string().contains("timestampColumn"), "{err}");
    }

    #[test]
    fn kusto_source_requires_timeseries() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"entities":{"Study":{"source":{"type":"KustoTable","clusterUri":"https://x.kusto","databaseName":"db"}}}}"#,
        ).unwrap();
        let err = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap_err();
        assert!(err.to_string().contains("KustoTable"), "{err}");
    }

    #[test]
    fn kusto_timeseries_binding_matches_schema() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"entities":{"Study":{"dataBindingType":"TimeSeries","timestampColumn":"ts","source":{"type":"KustoTable","clusterUri":"https://x.kusto.windows.net","databaseName":"telemetry"}}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();
        let v = payload(find_part(&parts, "EntityTypes/8880000000001/DataBindings"));
        assert_schema_valid("dataBinding", &v);
        let stp = &v["dataBindingConfiguration"]["sourceTableProperties"];
        assert_eq!(stp["sourceType"], "KustoTable");
        assert_eq!(stp["clusterUri"], "https://x.kusto.windows.net");
        assert_eq!(stp["databaseName"], "telemetry");
        // sourceType must remain the first key for the Kusto variant too.
        let content = &find_part(&parts, "DataBindings").content;
        let st = content.find("\"sourceType\"").unwrap();
        assert!(st < content.find("\"clusterUri\"").unwrap());
    }

    #[test]
    fn kusto_contextualization_rejected() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"has site":{"table":"site","sourceColumns":["study_id"],"targetColumns":["siteId"],"source":{"type":"KustoTable","clusterUri":"https://x","databaseName":"db"}}}}"#,
        ).unwrap();
        let err = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap_err();
        assert!(err.to_string().contains("LakehouseTable"), "{err}");
    }

    #[test]
    fn relationship_without_binding_emits_type_only() {
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"other":{"table":"t","sourceColumns":["a"],"targetColumns":["b"]}}}"#,
        ).unwrap();
        let parts = generate_fabric_parts(&binding_model(), Some(&lakehouse_ctx(spec))).unwrap();
        assert!(
            parts
                .iter()
                .any(|p| p.path == "RelationshipTypes/9990000000001/definition.json")
        );
        assert!(
            !parts
                .iter()
                .any(|p| p.path.contains("9990000000001/Contextualizations"))
        );
    }

    #[test]
    fn no_binding_context_emits_schema_only() {
        let parts = generate_fabric_parts(&binding_model(), None).unwrap();
        assert!(!parts.iter().any(|p| p.path.contains("DataBindings")));
        assert!(!parts.iter().any(|p| p.path.contains("Contextualizations")));
    }

    #[test]
    fn sanitize_name_matches_schema_pattern() {
        assert_eq!(sanitize_name("has site"), "has_site");
        assert_eq!(
            sanitize_name("drives  manufacturing "),
            "drives_manufacturing"
        );
        assert_eq!(sanitize_name("Study"), "Study");
        assert_eq!(sanitize_name("123bad"), "type_123bad");
        let re = regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{0,127}$").unwrap();
        for label in ["has site", "drives  manufacturing", "9lives", "  spaced  "] {
            assert!(re.is_match(&sanitize_name(label)), "label {label:?}");
        }
    }

    #[test]
    fn deterministic_uuid_stable_and_valid() {
        let a = deterministic_uuid("binding", "Study");
        assert_eq!(a, deterministic_uuid("binding", "Study"));
        assert_ne!(a, deterministic_uuid("binding", "Site"));
        let segs: Vec<&str> = a.split('-').collect();
        assert_eq!(
            segs.iter().map(|s| s.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
    }

    #[test]
    fn resolve_binding_context_none_when_unset() {
        assert!(
            resolve_binding_context(Some("ws"), None, None, None, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn resolve_binding_context_defaults_from_flags() {
        let ctx = resolve_binding_context(Some("ws-flag"), Some("lh-flag"), None, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(ctx.default_item_id.as_deref(), Some("lh-flag"));
        assert_eq!(ctx.default_workspace_id.as_deref(), Some("ws-flag"));
    }

    #[test]
    fn resolve_source_precedence_and_defaults() {
        let ctx = lakehouse_ctx(BindingSpec::default());
        match ctx.resolve_source(None).unwrap() {
            ResolvedSource::Lakehouse {
                workspace_id,
                source_schema,
                ..
            } => {
                assert_eq!(workspace_id, WS);
                assert_eq!(source_schema.as_deref(), Some("dbo"));
            }
            ResolvedSource::Kusto { .. } => panic!("expected Lakehouse"),
        }
        let local: SourceSpec =
            serde_json::from_str(r#"{"workspaceId":"ws-override","sourceSchema":"gold"}"#).unwrap();
        match ctx.resolve_source(Some(&local)).unwrap() {
            ResolvedSource::Lakehouse {
                workspace_id,
                source_schema,
                ..
            } => {
                assert_eq!(workspace_id, "ws-override");
                assert_eq!(source_schema.as_deref(), Some("gold"));
            }
            ResolvedSource::Kusto { .. } => panic!("expected Lakehouse"),
        }
    }

    #[test]
    fn resolve_source_missing_item_errors() {
        let ctx = BindingContext {
            default_workspace_id: Some(WS.into()),
            default_item_id: None,
            default_schema: None,
            spec: BindingSpec::default(),
        };
        let err = ctx.resolve_source(None).unwrap_err();
        assert!(err.to_string().contains("item ID"), "{err}");
    }

    #[test]
    fn resolve_binding_context_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.json");
        std::fs::write(&file, "{ not json").unwrap();
        let err =
            resolve_binding_context(Some("ws"), Some("lh"), None, None, file.to_str()).unwrap_err();
        assert!(err.to_string().contains("Invalid binding map JSON"));
    }

    // -----------------------------------------------------------------------
    // `ontology bind` — bind live types by name
    // -----------------------------------------------------------------------

    /// Build fake getDefinition parts for an ontology:
    /// Study(E1: studyId id, studyName) --`has_site(R1)`--> Site(E2: siteId id).
    fn live_def_parts() -> Vec<Value> {
        fn part(path: &str, body: &Value) -> Value {
            serde_json::json!({
                "path": path,
                "payload": BASE64.encode(serde_json::to_vec(body).unwrap()),
                "payloadType": "InlineBase64",
            })
        }
        vec![
            part("definition.json", &serde_json::json!({})),
            part(
                "EntityTypes/E1/definition.json",
                &serde_json::json!({
                    "id": "E1", "namespace": "usertypes", "name": "Study", "namespaceType": "Custom",
                    "entityIdParts": ["P1"],
                    "properties": [
                        {"id": "P1", "name": "studyId", "valueType": "String"},
                        {"id": "P2", "name": "studyName", "valueType": "String"}
                    ]
                }),
            ),
            part(
                "EntityTypes/E2/definition.json",
                &serde_json::json!({
                    "id": "E2", "namespace": "usertypes", "name": "Site", "namespaceType": "Custom",
                    "entityIdParts": ["P3"],
                    "properties": [{"id": "P3", "name": "siteId", "valueType": "String"}]
                }),
            ),
            part(
                "RelationshipTypes/R1/definition.json",
                &serde_json::json!({
                    "id": "R1", "namespace": "usertypes", "name": "has_site", "namespaceType": "Custom",
                    "source": {"entityTypeId": "E1"}, "target": {"entityTypeId": "E2"}
                }),
            ),
        ]
    }

    #[test]
    fn parse_live_types_extracts_ids_props_endpoints() {
        let (entities, rels) = parse_live_types(&live_def_parts());
        assert_eq!(entities.len(), 2);
        assert_eq!(rels.len(), 1);
        let study = entities.iter().find(|e| e.name == "Study").unwrap();
        assert_eq!(study.id, "E1");
        assert_eq!(study.id_parts, vec!["P1"]);
        assert_eq!(
            study.properties,
            vec![
                ("P1".into(), "studyId".into()),
                ("P2".into(), "studyName".into())
            ]
        );
        assert_eq!(rels[0].source_type_id, "E1");
        assert_eq!(rels[0].target_type_id, "E2");
    }

    #[test]
    fn bind_generates_against_live_ids_and_validates() {
        let (entities, rels) = parse_live_types(&live_def_parts());
        // Map keyed with the OWL label "has site" must match live "has_site".
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"has site":{"table":"site","sourceColumns":["study_id"],"targetColumns":["siteId"]}}}"#,
        ).unwrap();
        let (parts, bound_e, bound_r) =
            generate_bindings_for_live(&entities, &rels, &lakehouse_ctx(spec)).unwrap();

        assert_eq!(bound_e, HashSet::from(["E1".to_string(), "E2".to_string()]));
        assert_eq!(bound_r, HashSet::from(["R1".to_string()]));

        // DataBinding for Study targets the LIVE property ids and sits under E1.
        let db = find_part(&parts, "EntityTypes/E1/DataBindings");
        let v = payload(db);
        assert_schema_valid("dataBinding", &v);
        let pbs = v["dataBindingConfiguration"]["propertyBindings"]
            .as_array()
            .unwrap();
        assert_eq!(pbs[0]["targetPropertyId"], "P1");
        assert_eq!(pbs[0]["sourceColumnName"], "studyId");

        // Contextualization keys map to live identifier property ids P1 / P3.
        let cx = payload(find_part(&parts, "RelationshipTypes/R1/Contextualizations"));
        assert_schema_valid("contextualization", &cx);
        assert_eq!(cx["sourceKeyRefBindings"][0]["targetPropertyId"], "P1");
        assert_eq!(
            cx["sourceKeyRefBindings"][0]["sourceColumnName"],
            "study_id"
        );
        assert_eq!(cx["targetKeyRefBindings"][0]["targetPropertyId"], "P3");
    }

    #[test]
    fn bind_entities_block_is_surgical() {
        let (entities, rels) = parse_live_types(&live_def_parts());
        let spec: BindingSpec =
            serde_json::from_str(r#"{"entities":{"Study":{"table":"study_dim"}}}"#).unwrap();
        let (_, bound_e, _) =
            generate_bindings_for_live(&entities, &rels, &lakehouse_ctx(spec)).unwrap();
        assert_eq!(
            bound_e,
            HashSet::from(["E1".to_string()]),
            "only Study bound"
        );
    }

    #[test]
    fn bind_binds_all_entities_when_no_entities_block() {
        let (entities, rels) = parse_live_types(&live_def_parts());
        let (_, bound_e, _) =
            generate_bindings_for_live(&entities, &rels, &lakehouse_ctx(BindingSpec::default()))
                .unwrap();
        assert_eq!(bound_e, HashSet::from(["E1".to_string(), "E2".to_string()]));
    }

    #[test]
    fn bind_strict_rejects_unknown_type_name() {
        let (entities, rels) = parse_live_types(&live_def_parts());
        let spec: BindingSpec = serde_json::from_str(
            r#"{"relationships":{"nope":{"table":"t","sourceColumns":["a"],"targetColumns":["b"]}}}"#,
        ).unwrap();
        let err = generate_bindings_for_live(&entities, &rels, &lakehouse_ctx(spec)).unwrap_err();
        assert!(err.to_string().contains("do not exist"), "{err}");
    }

    #[test]
    fn merge_replaces_bound_bindings_and_keeps_others() {
        // Existing definition already carries a stale DataBinding for E1 and an
        // unrelated Contextualization for R9 that must survive.
        let mut existing = live_def_parts();
        existing.push(serde_json::json!({
            "path": "EntityTypes/E1/DataBindings/stale-uuid.json",
            "payload": BASE64.encode(b"{}"), "payloadType": "InlineBase64"
        }));
        existing.push(serde_json::json!({
            "path": "RelationshipTypes/R9/Contextualizations/keep.json",
            "payload": BASE64.encode(b"{}"), "payloadType": "InlineBase64"
        }));

        let new_parts = vec![FabricPart {
            path: "EntityTypes/E1/DataBindings/new-uuid.json".to_string(),
            content: "{}".to_string(),
        }];
        let bound_e = HashSet::from(["E1".to_string()]);
        let merged = merge_definition_parts(&existing, new_parts, &bound_e, &HashSet::new());
        let paths: Vec<&str> = merged.iter().map(|p| p["path"].as_str().unwrap()).collect();

        assert!(
            !paths.contains(&"EntityTypes/E1/DataBindings/stale-uuid.json"),
            "stale dropped"
        );
        assert!(
            paths.contains(&"EntityTypes/E1/DataBindings/new-uuid.json"),
            "new added"
        );
        assert!(
            paths.contains(&"RelationshipTypes/R9/Contextualizations/keep.json"),
            "unrelated kept"
        );
        assert!(
            paths.contains(&"EntityTypes/E1/definition.json"),
            "type defs kept"
        );
    }

    #[test]
    fn name_matches_exact_and_sanitized() {
        assert!(name_matches("has_site", "has_site"));
        assert!(name_matches("has site", "has_site"));
        assert!(!name_matches("has site", "site"));
    }
}
