//! End-to-end integration tests for `fabio ontology` commands.
//!
//! Tests exercise the compiled binary against a live Microsoft Fabric tenant.
//! Requires valid Azure credentials and `FABIO_TEST_*` environment variables.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json, unique_name};
use serde_json::Value;
use serial_test::serial;

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["ontology", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should be an array (possibly empty)
    assert!(data.is_array());
    // count field must be present
    let _ = extract_count(&json);
}

// ---------------------------------------------------------------------------
// Create + Show + Update + Delete lifecycle
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_show_update_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_test");

    // Create ontology
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            "Test ontology for E2E",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Show ontology
    let assert = fabio()
        .args([
            "ontology",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], ont_id);
    assert_eq!(data["displayName"], name);

    // Update name and description
    let new_name = unique_name("ont_renamed");
    let assert = fabio()
        .args([
            "ontology",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--name",
            &new_name,
            "--description",
            "Updated description",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], new_name);

    // Delete (soft)
    let assert = fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["id"], ont_id);
}

// ---------------------------------------------------------------------------
// Create with --definition (JSON parts format)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_definition_json() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_def");

    // Create a definition JSON file with the mandatory definition.json part + TTL payload
    let ttl_content = "@prefix ex: <http://example.org/> .\nex:Thing a ex:Class .";
    let ttl_encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        ttl_content.as_bytes(),
    );
    let def_json_encoded =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"{}");

    let def = serde_json::json!({
        "parts": [
            {
                "path": "definition.json",
                "payload": def_json_encoded,
                "payloadType": "InlineBase64"
            },
            {
                "path": "ontology.ttl",
                "payload": ttl_encoded,
                "payloadType": "InlineBase64"
            }
        ]
    });

    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("definition.json");
    std::fs::write(&def_path, serde_json::to_string(&def).unwrap()).unwrap();

    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--definition",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Create with --file (auto-wraps TTL into definition)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_rdf_ttl() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_rdf");

    let dir = tempfile::tempdir().unwrap();
    let ttl_path = dir.path().join("schema.ttl");
    std::fs::write(
        &ttl_path,
        r#"@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix sales: <http://example.org/sales#> .

sales:SalesOntology a owl:Ontology ;
    rdfs:label "Sales Domain Ontology" ;
    rdfs:comment "Models customers, products, and orders for a retail domain." .

sales:Customer a owl:Class ;
    rdfs:label "Customer" ;
    rdfs:comment "A person or organization that purchases products." .

sales:Product a owl:Class ;
    rdfs:label "Product" ;
    rdfs:comment "An item available for sale." .

sales:Order a owl:Class ;
    rdfs:label "Order" ;
    rdfs:comment "A purchase transaction linking a customer to products." .

sales:hasName a owl:DatatypeProperty ;
    rdfs:domain sales:Customer ;
    rdfs:range xsd:string ;
    rdfs:label "name" .

sales:hasEmail a owl:DatatypeProperty ;
    rdfs:domain sales:Customer ;
    rdfs:range xsd:string ;
    rdfs:label "email" .

sales:hasPrice a owl:DatatypeProperty ;
    rdfs:domain sales:Product ;
    rdfs:range xsd:decimal ;
    rdfs:label "price" .

sales:placedBy a owl:ObjectProperty ;
    rdfs:domain sales:Order ;
    rdfs:range sales:Customer ;
    rdfs:label "placed by" .

sales:containsProduct a owl:ObjectProperty ;
    rdfs:domain sales:Order ;
    rdfs:range sales:Product ;
    rdfs:label "contains product" .

sales:orderDate a owl:DatatypeProperty ;
    rdfs:domain sales:Order ;
    rdfs:range xsd:date ;
    rdfs:label "order date" .
"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--file",
            ttl_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Create with --file OWL format
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_rdf_owl() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_owl");

    let dir = tempfile::tempdir().unwrap();
    let owl_path = dir.path().join("ontology.owl");
    std::fs::write(
        &owl_path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:xsd="http://www.w3.org/2001/XMLSchema#"
         xmlns:inv="http://example.org/inventory#">

  <owl:Ontology rdf:about="http://example.org/inventory">
    <rdfs:label>Inventory Management Ontology</rdfs:label>
    <rdfs:comment>Models warehouses, stock items, and supply chain relationships.</rdfs:comment>
  </owl:Ontology>

  <owl:Class rdf:about="http://example.org/inventory#Warehouse">
    <rdfs:label>Warehouse</rdfs:label>
    <rdfs:comment>A physical location where inventory is stored.</rdfs:comment>
  </owl:Class>

  <owl:Class rdf:about="http://example.org/inventory#StockItem">
    <rdfs:label>Stock Item</rdfs:label>
    <rdfs:comment>A product unit held in inventory.</rdfs:comment>
  </owl:Class>

  <owl:Class rdf:about="http://example.org/inventory#Supplier">
    <rdfs:label>Supplier</rdfs:label>
    <rdfs:comment>An entity that provides stock items.</rdfs:comment>
  </owl:Class>

  <owl:DatatypeProperty rdf:about="http://example.org/inventory#sku">
    <rdfs:domain rdf:resource="http://example.org/inventory#StockItem"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <rdfs:label>SKU</rdfs:label>
  </owl:DatatypeProperty>

  <owl:DatatypeProperty rdf:about="http://example.org/inventory#quantity">
    <rdfs:domain rdf:resource="http://example.org/inventory#StockItem"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#integer"/>
    <rdfs:label>quantity on hand</rdfs:label>
  </owl:DatatypeProperty>

  <owl:ObjectProperty rdf:about="http://example.org/inventory#storedIn">
    <rdfs:domain rdf:resource="http://example.org/inventory#StockItem"/>
    <rdfs:range rdf:resource="http://example.org/inventory#Warehouse"/>
    <rdfs:label>stored in</rdfs:label>
  </owl:ObjectProperty>

  <owl:ObjectProperty rdf:about="http://example.org/inventory#suppliedBy">
    <rdfs:domain rdf:resource="http://example.org/inventory#StockItem"/>
    <rdfs:range rdf:resource="http://example.org/inventory#Supplier"/>
    <rdfs:label>supplied by</rdfs:label>
  </owl:ObjectProperty>

</rdf:RDF>"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--file",
            owl_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Create with --file JSON-LD format
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_rdf_jsonld() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_jld");

    let dir = tempfile::tempdir().unwrap();
    let jsonld_path = dir.path().join("ontology.jsonld");
    std::fs::write(
        &jsonld_path,
        r#"{
  "@context": {
    "rdf": "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
    "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
    "owl": "http://www.w3.org/2002/07/owl#",
    "xsd": "http://www.w3.org/2001/XMLSchema#",
    "hr": "http://example.org/hr#"
  },
  "@graph": [
    {
      "@id": "hr:HROntology",
      "@type": "owl:Ontology",
      "rdfs:label": "Human Resources Ontology",
      "rdfs:comment": "Models employees, departments, and organizational structure."
    },
    {
      "@id": "hr:Employee",
      "@type": "owl:Class",
      "rdfs:label": "Employee",
      "rdfs:comment": "A person employed by the organization."
    },
    {
      "@id": "hr:Department",
      "@type": "owl:Class",
      "rdfs:label": "Department",
      "rdfs:comment": "An organizational unit within the company."
    },
    {
      "@id": "hr:Role",
      "@type": "owl:Class",
      "rdfs:label": "Role",
      "rdfs:comment": "A job function or position title."
    },
    {
      "@id": "hr:employeeId",
      "@type": "owl:DatatypeProperty",
      "rdfs:domain": {"@id": "hr:Employee"},
      "rdfs:range": {"@id": "xsd:string"},
      "rdfs:label": "employee ID"
    },
    {
      "@id": "hr:belongsToDepartment",
      "@type": "owl:ObjectProperty",
      "rdfs:domain": {"@id": "hr:Employee"},
      "rdfs:range": {"@id": "hr:Department"},
      "rdfs:label": "belongs to department"
    },
    {
      "@id": "hr:hasRole",
      "@type": "owl:ObjectProperty",
      "rdfs:domain": {"@id": "hr:Employee"},
      "rdfs:range": {"@id": "hr:Role"},
      "rdfs:label": "has role"
    },
    {
      "@id": "hr:reportsTo",
      "@type": "owl:ObjectProperty",
      "rdfs:domain": {"@id": "hr:Employee"},
      "rdfs:range": {"@id": "hr:Employee"},
      "rdfs:label": "reports to"
    }
  ]
}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--file",
            jsonld_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Hard delete
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_hard_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_hard");

    // Create
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Hard delete
    let assert = fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");

    // Verify it's gone (show should fail)
    fabio()
        .args([
            "ontology",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Get definition and update definition
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_get_and_update_definition() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_defn");

    // Create with initial RDF definition
    let dir = tempfile::tempdir().unwrap();
    let ttl_path = dir.path().join("initial.ttl");
    std::fs::write(
        &ttl_path,
        "@prefix ex: <http://example.org/> .\nex:A a ex:Class .",
    )
    .unwrap();

    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--file",
            ttl_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Get definition
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should contain a definition field or parts
    assert!(data.get("definition").is_some() || data.get("parts").is_some());

    // Update definition with new RDF via --file
    let updated_path = dir.path().join("updated.ttl");
    std::fs::write(
        &updated_path,
        "@prefix ex: <http://example.org/> .\nex:B a ex:Class .\nex:C a ex:Class .",
    )
    .unwrap();

    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            updated_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Update definition with JSON format (using --definition)
    let def_json_path = dir.path().join("def.json");
    let ttl_bytes = b"@prefix ex: <http://example.org/> .\nex:D a ex:Class .";
    let ttl_encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ttl_bytes);
    let def_json_encoded =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"{}");
    let def = serde_json::json!({
        "parts": [
            {
                "path": "definition.json",
                "payload": def_json_encoded,
                "payloadType": "InlineBase64"
            },
            {
                "path": "ontology.ttl",
                "payload": ttl_encoded,
                "payloadType": "InlineBase64"
            }
        ]
    });
    std::fs::write(&def_json_path, serde_json::to_string(&def).unwrap()).unwrap();

    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--definition",
            def_json_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Update definition via stdin
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_definition_from_stdin() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_stdin");

    // Create
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Update definition from stdin (using - as path)
    let ttl_bytes = b"@prefix ex: <http://example.org/> .\nex:StdinTest a ex:Class .";
    let ttl_encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ttl_bytes);
    let def_json_encoded =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"{}");
    let def_json = serde_json::json!({
        "parts": [
            {
                "path": "definition.json",
                "payload": def_json_encoded,
                "payloadType": "InlineBase64"
            },
            {
                "path": "ontology.ttl",
                "payload": ttl_encoded,
                "payloadType": "InlineBase64"
            }
        ]
    });
    let stdin_content = serde_json::to_string(&def_json).unwrap();

    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--definition",
            "-",
        ])
        .write_stdin(stdin_content)
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Update requires at least one field
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_requires_field() {
    let cfg = TestConfig::from_env();

    // Update without --name or --description should fail
    fabio()
        .args([
            "ontology",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Update-definition requires --definition or --file
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_definition_requires_source() {
    let cfg = TestConfig::from_env();

    // update-definition without --definition or --file should fail
    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// --definition and --file are mutually exclusive (create)
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn ontology_create_definition_and_rdf_conflict() {
    // This doesn't need a live tenant - clap should reject it
    fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            "fake-ws",
            "--name",
            "test",
            "--definition",
            "def.json",
            "--file",
            "schema.ttl",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

// ---------------------------------------------------------------------------
// --definition and --file are mutually exclusive (update-definition)
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn ontology_update_definition_and_rdf_conflict() {
    // This doesn't need a live tenant - clap should reject it
    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            "fake-ws",
            "--id",
            "fake-id",
            "--definition",
            "def.json",
            "--file",
            "schema.ttl",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

// ---------------------------------------------------------------------------
// Show non-existent ontology returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "ontology",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// List with --output table format
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_list_table_format() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "ontology",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--output",
            "table",
        ])
        .assert()
        .success();

    // Table output should contain header columns
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Table header should appear (or be empty for no items)
    assert!(stdout.contains("NAME") || stdout.is_empty() || stdout.contains("No items"));
}

// ---------------------------------------------------------------------------
// Create with --file unsupported extension
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_rdf_unsupported_extension() {
    let cfg = TestConfig::from_env();

    let dir = tempfile::tempdir().unwrap();
    let bad_path = dir.path().join("data.csv");
    std::fs::write(&bad_path, "a,b,c").unwrap();

    fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "should_fail",
            "--file",
            bad_path.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Update only description
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_description_only() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_desc");

    // Create
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Update description only
    let assert = fabio()
        .args([
            "ontology",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--description",
            "A new description",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Name should remain unchanged
    assert_eq!(data["displayName"], name);

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Update definition with --file (no --update-metadata to avoid needing .platform)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_definition_with_rdf() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_upd");

    // Create
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Update definition with --file
    let dir = tempfile::tempdir().unwrap();
    let ttl_path = dir.path().join("meta.ttl");
    std::fs::write(
        &ttl_path,
        "@prefix ex: <http://example.org/> .\nex:Meta a ex:Class .",
    )
    .unwrap();

    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            ttl_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Get definition with --decode flag
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_get_definition_decode() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_decode");

    // Create with entity type definition via --definition
    let entity_def = serde_json::json!({
        "id": "5550000000001",
        "namespace": "usertypes",
        "baseEntityTypeId": null,
        "name": "TestEntity",
        "entityIdParts": ["5550000000011"],
        "displayNamePropertyId": "5550000000011",
        "namespaceType": "Custom",
        "visibility": "Visible",
        "properties": [{
            "id": "5550000000011",
            "name": "DisplayName",
            "redefines": null,
            "baseTypeNamespaceType": null,
            "valueType": "String"
        }],
        "timeseriesProperties": []
    });

    let entity_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&entity_def).unwrap().as_bytes(),
    );
    let def_json_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"{}");

    let def = serde_json::json!({
        "parts": [
            {"path": "definition.json", "payload": def_json_b64, "payloadType": "InlineBase64"},
            {"path": "EntityTypes/5550000000001/definition.json", "payload": entity_b64, "payloadType": "InlineBase64"}
        ]
    });

    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("definition.json");
    std::fs::write(&def_path, serde_json::to_string(&def).unwrap()).unwrap();

    // Create ontology with definition
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--definition",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Get definition with --decode
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--decode",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    // Verify decoded payloads are present
    let mut found_entity = false;
    for part in parts {
        if part["path"].as_str().unwrap_or("").contains("EntityTypes/") {
            let decoded = &part["decodedPayload"];
            assert!(
                decoded.is_object(),
                "decodedPayload should be a JSON object"
            );
            assert_eq!(decoded["name"], "TestEntity");
            found_entity = true;
        }
    }
    assert!(found_entity, "Should find decoded entity type in parts");

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Create and update with --dir (directory-based definition)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_create_with_dir() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_dir");

    let dir = tempfile::tempdir().unwrap();
    let ont_dir = dir.path().join("ontology");
    std::fs::create_dir_all(ont_dir.join("EntityTypes").join("7770000000001")).unwrap();

    // definition.json
    std::fs::write(ont_dir.join("definition.json"), "{}").unwrap();

    // Entity type
    std::fs::write(
        ont_dir
            .join("EntityTypes")
            .join("7770000000001")
            .join("definition.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "id": "7770000000001",
            "namespace": "usertypes",
            "baseEntityTypeId": null,
            "name": "Machine",
            "entityIdParts": ["7770000000011"],
            "displayNamePropertyId": "7770000000011",
            "namespaceType": "Custom",
            "visibility": "Visible",
            "properties": [{
                "id": "7770000000011",
                "name": "DisplayName",
                "redefines": null,
                "baseTypeNamespaceType": null,
                "valueType": "String"
            }, {
                "id": "7770000000012",
                "name": "SerialNumber",
                "redefines": null,
                "baseTypeNamespaceType": null,
                "valueType": "String"
            }],
            "timeseriesProperties": []
        }))
        .unwrap(),
    )
    .unwrap();

    // Create with --dir
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            "Created from directory structure",
            "--dir",
            ont_dir.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Get definition and verify entity type was stored
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--decode",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    let entity_part = parts
        .iter()
        .find(|p| {
            p["path"]
                .as_str()
                .unwrap_or("")
                .contains("EntityTypes/7770000000001/definition.json")
        })
        .expect("EntityType part should exist");
    assert_eq!(entity_part["decodedPayload"]["name"], "Machine");

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Update definition with --dir (entity types + relationship + data binding)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_update_definition_with_dir() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_updir");

    // Create empty ontology
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Build directory with entity types, data binding, and relationship
    let dir = tempfile::tempdir().unwrap();
    let ont_dir = dir.path();

    // Entity type 1: Equipment
    let et1_dir = ont_dir.join("EntityTypes").join("8880000000001");
    std::fs::create_dir_all(et1_dir.join("DataBindings")).unwrap();
    std::fs::write(
        et1_dir.join("definition.json"),
        serde_json::to_string(&serde_json::json!({
            "id": "8880000000001",
            "namespace": "usertypes",
            "name": "Equipment",
            "entityIdParts": ["8880000000011"],
            "displayNamePropertyId": "8880000000011",
            "namespaceType": "Custom",
            "visibility": "Visible",
            "properties": [{
                "id": "8880000000011",
                "name": "DisplayName",
                "valueType": "String"
            }],
            "timeseriesProperties": []
        }))
        .unwrap(),
    )
    .unwrap();

    // Data binding for Equipment → sales table
    std::fs::write(
        et1_dir
            .join("DataBindings")
            .join("b0000001-0001-0001-0001-000000000001.json"),
        serde_json::to_string(&serde_json::json!({
            "id": "b0000001-0001-0001-0001-000000000001",
            "dataBindingConfiguration": {
                "dataBindingType": "NonTimeSeries",
                "propertyBindings": [{
                    "sourceColumnName": "country",
                    "targetPropertyId": "8880000000011"
                }],
                "sourceTableProperties": {
                    "sourceType": "LakehouseTable",
                    "workspaceId": cfg.source_workspace,
                    "itemId": cfg.source_lakehouse,
                    "sourceTableName": "sales",
                    "sourceSchema": "dbo"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Entity type 2: Sensor
    let et2_dir = ont_dir.join("EntityTypes").join("8880000000002");
    std::fs::create_dir_all(&et2_dir).unwrap();
    std::fs::write(
        et2_dir.join("definition.json"),
        serde_json::to_string(&serde_json::json!({
            "id": "8880000000002",
            "namespace": "usertypes",
            "name": "Sensor",
            "entityIdParts": ["8880000000021"],
            "displayNamePropertyId": "8880000000021",
            "namespaceType": "Custom",
            "visibility": "Visible",
            "properties": [{
                "id": "8880000000021",
                "name": "DisplayName",
                "valueType": "String"
            }],
            "timeseriesProperties": []
        }))
        .unwrap(),
    )
    .unwrap();

    // Relationship: Equipment hasSensor Sensor
    let rel_dir = ont_dir.join("RelationshipTypes").join("9990000000001");
    std::fs::create_dir_all(&rel_dir).unwrap();
    std::fs::write(
        rel_dir.join("definition.json"),
        serde_json::to_string(&serde_json::json!({
            "namespace": "usertypes",
            "id": "9990000000001",
            "name": "hasSensor",
            "namespaceType": "Custom",
            "source": {"entityTypeId": "8880000000001"},
            "target": {"entityTypeId": "8880000000002"}
        }))
        .unwrap(),
    )
    .unwrap();

    // Update definition from directory
    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--dir",
            ont_dir.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Verify with get-definition --decode
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--decode",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();

    assert!(
        paths.contains(&"EntityTypes/8880000000001/definition.json"),
        "Missing Equipment entity type"
    );
    assert!(
        paths.contains(&"EntityTypes/8880000000002/definition.json"),
        "Missing Sensor entity type"
    );
    assert!(
        paths.iter().any(|p| p.contains("DataBindings/")),
        "Missing data binding"
    );
    assert!(
        paths.contains(&"RelationshipTypes/9990000000001/definition.json"),
        "Missing relationship type"
    );

    // Verify Equipment entity type content
    let equipment_part = parts
        .iter()
        .find(|p| p["path"].as_str().unwrap_or("") == "EntityTypes/8880000000001/definition.json")
        .unwrap();
    assert_eq!(equipment_part["decodedPayload"]["name"], "Equipment");

    // Verify relationship content
    let rel_part = parts
        .iter()
        .find(|p| {
            p["path"].as_str().unwrap_or("") == "RelationshipTypes/9990000000001/definition.json"
        })
        .unwrap();
    assert_eq!(rel_part["decodedPayload"]["name"], "hasSensor");
    assert_eq!(
        rel_part["decodedPayload"]["source"]["entityTypeId"],
        "8880000000001"
    );

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// --dir and --definition/--file are mutually exclusive (create)
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn ontology_create_dir_conflicts_with_definition() {
    fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            "fake-ws",
            "--name",
            "test",
            "--dir",
            "/tmp",
            "--definition",
            "def.json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
#[serial]
fn ontology_create_dir_conflicts_with_file() {
    fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            "fake-ws",
            "--name",
            "test",
            "--dir",
            "/tmp",
            "--file",
            "schema.ttl",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

// ---------------------------------------------------------------------------
// --dir and --definition/--file are mutually exclusive (update-definition)
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn ontology_update_definition_dir_conflicts_with_definition() {
    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            "fake-ws",
            "--id",
            "fake-id",
            "--dir",
            "/tmp",
            "--definition",
            "def.json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
#[serial]
fn ontology_update_definition_dir_conflicts_with_file() {
    fabio()
        .args([
            "ontology",
            "update-definition",
            "--workspace",
            "fake-ws",
            "--id",
            "fake-id",
            "--dir",
            "/tmp",
            "--file",
            "schema.ttl",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

// ---------------------------------------------------------------------------
// Full IoT scenario: create ontology with entity types + data bindings
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_iot_scenario_entity_types_and_data_bindings() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_iot");

    // Build ontology definition with entity types bound to lakehouse sales table
    let entity_def = serde_json::json!({
        "id": "6660000000001",
        "namespace": "usertypes",
        "baseEntityTypeId": null,
        "name": "SalesRecord",
        "entityIdParts": ["6660000000011"],
        "displayNamePropertyId": "6660000000011",
        "namespaceType": "Custom",
        "visibility": "Visible",
        "properties": [
            {"id": "6660000000011", "name": "DisplayName", "redefines": null, "baseTypeNamespaceType": null, "valueType": "String"},
            {"id": "6660000000012", "name": "Country", "redefines": null, "baseTypeNamespaceType": null, "valueType": "String"},
            {"id": "6660000000013", "name": "Revenue", "redefines": null, "baseTypeNamespaceType": null, "valueType": "Double"}
        ],
        "timeseriesProperties": []
    });

    let binding_def = serde_json::json!({
        "id": "c0000001-0001-0001-0001-000000000001",
        "dataBindingConfiguration": {
            "dataBindingType": "NonTimeSeries",
            "propertyBindings": [
                {"sourceColumnName": "country", "targetPropertyId": "6660000000011"},
                {"sourceColumnName": "country", "targetPropertyId": "6660000000012"},
                {"sourceColumnName": "revenue", "targetPropertyId": "6660000000013"}
            ],
            "sourceTableProperties": {
                "sourceType": "LakehouseTable",
                "workspaceId": &cfg.source_workspace,
                "itemId": &cfg.source_lakehouse,
                "sourceTableName": "sales",
                "sourceSchema": "dbo"
            }
        }
    });

    let def_json_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"{}");
    let entity_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&entity_def).unwrap().as_bytes(),
    );
    let binding_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&binding_def).unwrap().as_bytes(),
    );

    let full_def = serde_json::json!({
        "parts": [
            {"path": "definition.json", "payload": def_json_b64, "payloadType": "InlineBase64"},
            {"path": "EntityTypes/6660000000001/definition.json", "payload": entity_b64, "payloadType": "InlineBase64"},
            {"path": "EntityTypes/6660000000001/DataBindings/c0000001-0001-0001-0001-000000000001.json", "payload": binding_b64, "payloadType": "InlineBase64"}
        ]
    });

    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("def.json");
    std::fs::write(&def_path, serde_json::to_string(&full_def).unwrap()).unwrap();

    // Create ontology with entity types + data bindings
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            "IoT scenario with entity types and data bindings",
            "--definition",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Verify entity types exist in definition
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--decode",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    // Verify SalesRecord entity type
    let entity_part = parts
        .iter()
        .find(|p| {
            p["path"]
                .as_str()
                .unwrap_or("")
                .contains("EntityTypes/6660000000001/definition.json")
        })
        .expect("SalesRecord entity type should exist");
    assert_eq!(entity_part["decodedPayload"]["name"], "SalesRecord");
    let properties = entity_part["decodedPayload"]["properties"]
        .as_array()
        .unwrap();
    assert_eq!(properties.len(), 3);

    // Verify data binding exists
    let binding_part = parts
        .iter()
        .find(|p| p["path"].as_str().unwrap_or("").contains("DataBindings/"))
        .expect("Data binding should exist");
    let binding_config = &binding_part["decodedPayload"]["dataBindingConfiguration"];
    assert_eq!(binding_config["dataBindingType"], "NonTimeSeries");
    assert_eq!(
        binding_config["sourceTableProperties"]["sourceTableName"],
        "sales"
    );

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Get definition without --decode (original behavior preserved)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_get_definition_without_decode() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_nodec");

    // Create
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Get definition WITHOUT --decode
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    // Without --decode, parts should NOT have decodedPayload field
    for part in parts {
        assert!(
            part.get("decodedPayload").is_none(),
            "decodedPayload should not exist without --decode flag"
        );
        // But payload should be base64 string
        assert!(part["payload"].is_string());
    }

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
}

// ─── Import Tests ────────────────────────────────────────────────────────────

#[test]
fn ontology_import_rdf_to_directory() {
    let rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:xsd="http://www.w3.org/2001/XMLSchema#"
         xmlns:ont="http://example.org/">
  <owl:Class rdf:about="http://example.org/Sensor"><rdfs:label>Sensor</rdfs:label></owl:Class>
  <owl:Class rdf:about="http://example.org/Reading"><rdfs:label>Reading</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/sensor_id">
    <rdfs:label>sensorId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Sensor"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:ObjectProperty rdf:about="http://example.org/produces">
    <rdfs:label>produces</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Sensor"/>
    <rdfs:range rdf:resource="http://example.org/Reading"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;

    let tmp_dir = std::env::temp_dir();
    let rdf_file = tmp_dir.join("fabio_test_import.rdf");
    let out_dir = tmp_dir.join("fabio_test_import_out");
    std::fs::write(&rdf_file, rdf).unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--file",
            &rdf_file.display().to_string(),
            "--output-dir",
            &out_dir.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);
    assert!(out_dir.join("definition.json").exists());
    assert!(out_dir.join("EntityTypes").exists());
    assert!(out_dir.join("RelationshipTypes").exists());

    let _ = std::fs::remove_file(&rdf_file);
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn ontology_import_jsonld_to_directory() {
    let jsonld = r#"{"@graph": [
        {"@id": "http://ex.org/Device", "@type": "owl:Class", "rdfs:label": "Device"},
        {"@id": "http://ex.org/Event", "@type": "owl:Class", "rdfs:label": "Event"},
        {"@id": "http://ex.org/emits", "@type": "owl:ObjectProperty", "rdfs:label": "emits",
         "rdfs:domain": {"@id": "http://ex.org/Device"}, "rdfs:range": {"@id": "http://ex.org/Event"}}
    ]}"#;

    let tmp_dir = std::env::temp_dir();
    let file = tmp_dir.join("fabio_test_import.jsonld");
    let out_dir = tmp_dir.join("fabio_test_import_jsonld_out");
    std::fs::write(&file, jsonld).unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--file",
            &file.display().to_string(),
            "--output-dir",
            &out_dir.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);

    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn ontology_import_dry_run() {
    let rdf = r#"<?xml version="1.0"?><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:owl="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"><owl:Class rdf:about="http://ex.org/T"><rdfs:label>T</rdfs:label></owl:Class></rdf:RDF>"#;
    let tmp = std::env::temp_dir().join("fabio_test_import_dryrun.rdf");
    std::fs::write(&tmp, rdf).unwrap();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--file",
            &tmp.display().to_string(),
            "--output-dir",
            "/tmp/unused",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["entity_types"], 1);

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn ontology_import_missing_file_fails() {
    fabio()
        .args([
            "ontology",
            "import",
            "--file",
            "/nonexistent/path.rdf",
            "--output-dir",
            "/tmp/x",
        ])
        .assert()
        .failure();
}

#[test]
fn ontology_import_no_output_no_workspace_fails() {
    let tmp = std::env::temp_dir().join("fabio_test_noout.rdf");
    std::fs::write(&tmp, "<?xml version=\"1.0\"?><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/>").unwrap();

    let output = fabio()
        .args(["ontology", "import", "--file", &tmp.display().to_string()])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("--workspace") || stderr.contains("--output-dir"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_import_rdf_live() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_import");

    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let ont_id = extract_data(&json)["id"].as_str().unwrap().to_string();

    let rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:owl="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#" xmlns:xsd="http://www.w3.org/2001/XMLSchema#" xmlns:ont="http://example.org/">
  <owl:Class rdf:about="http://example.org/Vehicle"><rdfs:label>Vehicle</rdfs:label></owl:Class>
  <owl:Class rdf:about="http://example.org/Trip"><rdfs:label>Trip</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/vehicle_vin">
    <rdfs:label>vin</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Vehicle"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/vehicle_name">
    <rdfs:label>name</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Vehicle"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/trip_id">
    <rdfs:label>tripId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Trip"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/trip_name">
    <rdfs:label>destination</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Trip"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
  </owl:DatatypeProperty>
  <owl:ObjectProperty rdf:about="http://example.org/makes">
    <rdfs:label>makes</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Vehicle"/>
    <rdfs:range rdf:resource="http://example.org/Trip"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;
    let tmp = std::env::temp_dir().join("fabio_e2e_import.rdf");
    std::fs::write(&tmp, rdf).unwrap();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "imported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_import_jsonld_live() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_import_jld");

    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let ont_id = extract_data(&json)["id"].as_str().unwrap().to_string();

    let jsonld = r#"{"@graph": [
        {"@id": "http://ex.org/Machine", "@type": "owl:Class", "rdfs:label": "Machine"},
        {"@id": "http://ex.org/Alert", "@type": "owl:Class", "rdfs:label": "Alert"},
        {"@id": "http://ex.org/machine_id", "@type": "owl:DatatypeProperty", "rdfs:label": "machineId",
         "rdfs:domain": {"@id": "http://ex.org/Machine"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/machine_name", "@type": "owl:DatatypeProperty", "rdfs:label": "name",
         "rdfs:domain": {"@id": "http://ex.org/Machine"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"}},
        {"@id": "http://ex.org/alert_id", "@type": "owl:DatatypeProperty", "rdfs:label": "alertId",
         "rdfs:domain": {"@id": "http://ex.org/Alert"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/alert_severity", "@type": "owl:DatatypeProperty", "rdfs:label": "severity",
         "rdfs:domain": {"@id": "http://ex.org/Alert"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#integer"}},
        {"@id": "http://ex.org/triggers", "@type": "owl:ObjectProperty", "rdfs:label": "triggers",
         "rdfs:domain": {"@id": "http://ex.org/Machine"}, "rdfs:range": {"@id": "http://ex.org/Alert"}}
    ]}"#;
    let tmp = std::env::temp_dir().join("fabio_e2e_import.jsonld");
    std::fs::write(&tmp, jsonld).unwrap();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "imported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp);
}

// ─── Export Tests ────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_export_rdf_live() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_export_rdf");

    // 1. Create ontology + import some data
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let rdf_in = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:owl="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#" xmlns:xsd="http://www.w3.org/2001/XMLSchema#" xmlns:ont="http://example.org/">
  <owl:Class rdf:about="http://example.org/Pump"><rdfs:label>Pump</rdfs:label></owl:Class>
  <owl:Class rdf:about="http://example.org/Alarm"><rdfs:label>Alarm</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/pump_id">
    <rdfs:label>pumpId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Pump"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/pump_name">
    <rdfs:label>name</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Pump"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/alarm_id">
    <rdfs:label>alarmId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Alarm"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/alarm_level">
    <rdfs:label>level</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Alarm"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#integer"/>
    <ont:propertyType>integer</ont:propertyType>
  </owl:DatatypeProperty>
  <owl:ObjectProperty rdf:about="http://example.org/triggers">
    <rdfs:label>triggers</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Pump"/>
    <rdfs:range rdf:resource="http://example.org/Alarm"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;
    let tmp_in = std::env::temp_dir().join("fabio_e2e_export_in.rdf");
    std::fs::write(&tmp_in, rdf_in).unwrap();

    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp_in.display().to_string(),
        ])
        .assert()
        .success();

    // 2. Export as RDF
    let tmp_out = std::env::temp_dir().join("fabio_e2e_export_out.rdf");
    let output = fabio()
        .args([
            "ontology",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--format",
            "rdf",
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);

    // 3. Verify exported file contains OWL elements
    let content = std::fs::read_to_string(&tmp_out).unwrap();
    assert!(content.contains("owl:Class"), "Should contain owl:Class");
    assert!(content.contains("Pump"), "Should contain Pump entity");
    assert!(content.contains("Alarm"), "Should contain Alarm entity");
    assert!(
        content.contains("owl:ObjectProperty"),
        "Should contain relationship"
    );
    assert!(
        content.contains("owl:DatatypeProperty"),
        "Should contain properties"
    );

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&tmp_out);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_export_jsonld_live() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_export_jld");

    // 1. Create + import
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let jsonld_in = r#"{"@graph": [
        {"@id": "http://ex.org/Sensor", "@type": "owl:Class", "rdfs:label": "Sensor"},
        {"@id": "http://ex.org/Metric", "@type": "owl:Class", "rdfs:label": "Metric"},
        {"@id": "http://ex.org/sensor_id", "@type": "owl:DatatypeProperty", "rdfs:label": "sensorId",
         "rdfs:domain": {"@id": "http://ex.org/Sensor"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/sensor_name", "@type": "owl:DatatypeProperty", "rdfs:label": "name",
         "rdfs:domain": {"@id": "http://ex.org/Sensor"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"}},
        {"@id": "http://ex.org/metric_id", "@type": "owl:DatatypeProperty", "rdfs:label": "metricId",
         "rdfs:domain": {"@id": "http://ex.org/Metric"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/metric_value", "@type": "owl:DatatypeProperty", "rdfs:label": "value",
         "rdfs:domain": {"@id": "http://ex.org/Metric"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#decimal"}},
        {"@id": "http://ex.org/emits", "@type": "owl:ObjectProperty", "rdfs:label": "emits",
         "rdfs:domain": {"@id": "http://ex.org/Sensor"}, "rdfs:range": {"@id": "http://ex.org/Metric"}}
    ]}"#;
    let tmp_in = std::env::temp_dir().join("fabio_e2e_export_jld_in.jsonld");
    std::fs::write(&tmp_in, jsonld_in).unwrap();

    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp_in.display().to_string(),
        ])
        .assert()
        .success();

    // 2. Export as JSON-LD
    let tmp_out = std::env::temp_dir().join("fabio_e2e_export_out.jsonld");
    let output = fabio()
        .args([
            "ontology",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--format",
            "jsonld",
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert_eq!(data["entity_types"], 2);
    assert_eq!(data["relationship_types"], 1);

    // 3. Verify JSON-LD structure
    let content = std::fs::read_to_string(&tmp_out).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(doc.get("@context").is_some(), "Should have @context");
    let graph = doc["@graph"].as_array().unwrap();
    let class_count = graph.iter().filter(|n| n["@type"] == "owl:Class").count();
    assert_eq!(class_count, 2);

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&tmp_out);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_round_trip_rdf() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_roundtrip");

    // 1. Create ontology + import RDF
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let original_rdf = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:owl="http://www.w3.org/2002/07/owl#" xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#" xmlns:xsd="http://www.w3.org/2001/XMLSchema#" xmlns:ont="http://example.org/">
  <owl:Class rdf:about="http://example.org/Widget"><rdfs:label>Widget</rdfs:label></owl:Class>
  <owl:Class rdf:about="http://example.org/Factory"><rdfs:label>Factory</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/widget_id">
    <rdfs:label>widgetId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Widget"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/widget_name">
    <rdfs:label>name</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Widget"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/factory_id">
    <rdfs:label>factoryId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Factory"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/factory_location">
    <rdfs:label>location</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Factory"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
  </owl:DatatypeProperty>
  <owl:ObjectProperty rdf:about="http://example.org/madeIn">
    <rdfs:label>madeIn</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/Widget"/>
    <rdfs:range rdf:resource="http://example.org/Factory"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;
    let tmp_in = std::env::temp_dir().join("fabio_e2e_roundtrip_in.rdf");
    std::fs::write(&tmp_in, original_rdf).unwrap();

    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp_in.display().to_string(),
        ])
        .assert()
        .success();

    // 2. Export back to RDF
    let tmp_out = std::env::temp_dir().join("fabio_e2e_roundtrip_out.rdf");
    fabio()
        .args([
            "ontology",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--format",
            "rdf",
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();

    // 3. Re-import the exported RDF into a new ontology to verify it's valid
    let name2 = unique_name("ont_roundtrip2");
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name2,
        ])
        .assert()
        .success();
    let ont_id2 = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id2,
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "imported");
    assert_eq!(
        data["entity_types"], 2,
        "Round-trip should preserve 2 entity types"
    );
    assert_eq!(
        data["relationship_types"], 1,
        "Round-trip should preserve 1 relationship"
    );

    // Cleanup both ontologies
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id2,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&tmp_out);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_round_trip_jsonld() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_rt_jld");

    // 1. Create ontology + import JSON-LD
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let jsonld_in = r#"{"@graph": [
        {"@id": "http://ex.org/Robot", "@type": "owl:Class", "rdfs:label": "Robot"},
        {"@id": "http://ex.org/Task", "@type": "owl:Class", "rdfs:label": "Task"},
        {"@id": "http://ex.org/robot_serial", "@type": "owl:DatatypeProperty", "rdfs:label": "serial",
         "rdfs:domain": {"@id": "http://ex.org/Robot"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/robot_model", "@type": "owl:DatatypeProperty", "rdfs:label": "model",
         "rdfs:domain": {"@id": "http://ex.org/Robot"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"}},
        {"@id": "http://ex.org/task_id", "@type": "owl:DatatypeProperty", "rdfs:label": "taskId",
         "rdfs:domain": {"@id": "http://ex.org/Task"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
         "ont:isIdentifier": true},
        {"@id": "http://ex.org/task_priority", "@type": "owl:DatatypeProperty", "rdfs:label": "priority",
         "rdfs:domain": {"@id": "http://ex.org/Task"}, "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#integer"}},
        {"@id": "http://ex.org/executes", "@type": "owl:ObjectProperty", "rdfs:label": "executes",
         "rdfs:domain": {"@id": "http://ex.org/Robot"}, "rdfs:range": {"@id": "http://ex.org/Task"}}
    ]}"#;
    let tmp_in = std::env::temp_dir().join("fabio_e2e_rt_jld_in.jsonld");
    std::fs::write(&tmp_in, jsonld_in).unwrap();

    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp_in.display().to_string(),
        ])
        .assert()
        .success();

    // 2. Export as JSON-LD
    let tmp_out = std::env::temp_dir().join("fabio_e2e_rt_jld_out.jsonld");
    fabio()
        .args([
            "ontology",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--format",
            "jsonld",
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();

    // 3. Re-import exported JSON-LD into a new ontology
    let name2 = unique_name("ont_rt_jld2");
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name2,
        ])
        .assert()
        .success();
    let ont_id2 = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id2,
            "--file",
            &tmp_out.display().to_string(),
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "imported");
    assert_eq!(
        data["entity_types"], 2,
        "Round-trip should preserve 2 entity types"
    );
    assert_eq!(
        data["relationship_types"], 1,
        "Round-trip should preserve 1 relationship"
    );

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id2,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&tmp_out);
}

// ─── Full Format Tests ───────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_full_format_importable() {
    let cfg = TestConfig::from_env();

    // 1. Export tenant as full RDF
    let tmp_rdf = std::env::temp_dir().join("fabio_e2e_full.rdf");
    let output = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &cfg.source_workspace,
            "--format",
            "full",
            "--output-file",
            &tmp_rdf.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "written");
    assert_eq!(data["format"], "full");
    assert!(data["nodes"].as_u64().unwrap() > 0);

    // 2. Verify file contains both schema and instances
    let content = std::fs::read_to_string(&tmp_rdf).unwrap();
    assert!(content.contains("owl:Class"), "Should have schema classes");
    assert!(
        content.contains("rdf:Description"),
        "Should have instance data"
    );
    assert!(
        content.contains("owl:ObjectProperty"),
        "Should have relationships"
    );

    // 3. Import the schema part into a Fabric Ontology
    let name = unique_name("ont_full_fmt");
    let output = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&output))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--file",
            &tmp_rdf.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "imported");
    assert!(
        data["entity_types"].as_u64().unwrap() > 0,
        "Should import entity types from full RDF"
    );

    // Cleanup
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp_rdf);
}

#[test]
fn context_tenant_full_format_dry_run_produces_correct_structure() {
    // Dry-run with full format should still work (no API call, but validates arg parsing)
    fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "full",
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_owl_format_valid_structure() {
    let cfg = TestConfig::from_env();
    let tmp = std::env::temp_dir().join("fabio_e2e_owl_format.jsonld");

    let output = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &cfg.source_workspace,
            "--format",
            "owl",
            "--output-file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["format"], "owl");
    assert!(data["nodes"].as_u64().unwrap() > 0);

    // Verify it's valid OWL JSON-LD
    let content = std::fs::read_to_string(&tmp).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(doc.get("@context").is_some(), "Should have @context");
    let graph = doc["@graph"].as_array().unwrap();
    let has_owl_class = graph.iter().any(|n| n["@type"] == "owl:Class");
    let has_owl_prop = graph.iter().any(|n| n["@type"] == "owl:DatatypeProperty");
    assert!(has_owl_class, "Should have owl:Class nodes");
    assert!(has_owl_prop, "Should have owl:DatatypeProperty nodes");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_rdf_format_valid_structure() {
    let cfg = TestConfig::from_env();
    let tmp = std::env::temp_dir().join("fabio_e2e_rdf_format.rdf");

    let output = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &cfg.source_workspace,
            "--format",
            "rdf",
            "--output-file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["format"], "rdf");

    // Verify it's valid RDF/XML with OWL elements
    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.starts_with("<?xml"), "Should be XML");
    assert!(content.contains("owl:Class"), "Should have owl:Class");
    assert!(
        content.contains("owl:DatatypeProperty"),
        "Should have properties"
    );
    assert!(content.contains("rdf:RDF"), "Should have RDF root");
    // Should NOT have instance data (that's the 'full' format)
    assert!(
        !content.contains("rdf:Description"),
        "Schema-only format should not have instances"
    );

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// Offline: import without a data source hints how to bind next
// ---------------------------------------------------------------------------

#[test]
fn ontology_import_without_binding_emits_next_step_hint() {
    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("t.rdf");
    std::fs::write(
        &rdf,
        r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#">
  <owl:Class rdf:about="http://ex.org/Thing"><rdfs:label>Thing</rdfs:label></owl:Class>
</rdf:RDF>"#,
    )
    .unwrap();

    // No data source -> schema only, hint present.
    let out = dir.path().join("schema-only");
    let assert = fabio()
        .args([
            "ontology",
            "import",
            "--file",
            rdf.to_str().unwrap(),
            "--output-dir",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["bindings"], 0);
    assert!(
        data["hint"]
            .as_str()
            .unwrap_or("")
            .contains("ontology bind"),
        "expected a bind next-step hint, got: {data}"
    );

    // With a Lakehouse source -> bindings generated, no hint.
    let out2 = dir.path().join("bound");
    let assert = fabio()
        .args([
            "ontology",
            "import",
            "--file",
            rdf.to_str().unwrap(),
            "--lakehouse",
            "22222222-2222-4222-8222-222222222222",
            "--lakehouse-workspace",
            "11111111-1111-4111-8111-111111111111",
            "--output-dir",
            out2.to_str().unwrap(),
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["bindings"], 1);
    assert!(data.get("hint").is_none(), "no hint when bound: {data}");
}

// ===========================================================================
// New-feature E2E coverage (this session): schema-aligned bindings, inheritance,
// time-series/untyped properties, multiple bindings, Documents/ResourceLinks,
// `ontology bind`, the schema-only hint, and the --lakehouse-schema flag.
// ===========================================================================

/// A rich OWL model exercising inheritance, ont:isTimeSeries, ont:isUntyped.
const FEATURES_RDF: &str = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:ont="http://example.org/dt/">
  <owl:Class rdf:about="http://example.org/dt/Asset"><rdfs:label>Asset</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/assetId">
    <rdfs:label>assetId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Asset"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:Class rdf:about="http://example.org/dt/Sensor">
    <rdfs:label>Sensor</rdfs:label>
    <rdfs:subClassOf rdf:resource="http://example.org/dt/Asset"/>
  </owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/sensorId">
    <rdfs:label>sensorId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Sensor"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/temperature">
    <rdfs:label>temperature</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Sensor"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#double"/>
    <ont:isTimeSeries rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isTimeSeries>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/reading_ts">
    <rdfs:label>reading_ts</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Sensor"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#dateTime"/>
    <ont:isTimeSeries rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isTimeSeries>
  </owl:DatatypeProperty>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/payload">
    <rdfs:label>payload</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Sensor"/>
    <ont:isUntyped rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isUntyped>
  </owl:DatatypeProperty>
  <owl:ObjectProperty rdf:about="http://example.org/dt/monitors">
    <rdfs:label>monitors</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Sensor"/>
    <rdfs:range rdf:resource="http://example.org/dt/Asset"/>
  </owl:ObjectProperty>
</rdf:RDF>"#;

fn decoded_parts(json: &Value) -> Vec<(String, Value)> {
    json["data"]["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["path"].as_str().unwrap_or("").to_string(),
                p["decodedPayload"].clone(),
            )
        })
        .collect()
}

fn part<'a>(parts: &'a [(String, Value)], needle: &str) -> &'a Value {
    &parts
        .iter()
        .find(|(path, _)| path.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "no part '{needle}'; have: {:?}",
                parts.iter().map(|(p, _)| p).collect::<Vec<_>>()
            )
        })
        .1
}

fn delete_ontology(ws: &str, id: &str) {
    let _ = fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            ws,
            "--id",
            id,
            "--hard",
        ])
        .assert();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn e2e_ontology_import_full_features_roundtrip() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    std::fs::write(&rdf, FEATURES_RDF).unwrap();
    let map = dir.path().join("bindings.json");
    std::fs::write(
        &map,
        serde_json::to_string(&serde_json::json!({
            "entities": {
                "Asset": { "table": "asset" },
                "Sensor": {
                    "documents": [{"displayText": "Manual", "url": "https://example.org/manual"}],
                    "bindings": [
                        {"table": "sensor_static", "dataBindingType": "NonTimeSeries", "properties": ["sensorId", "payload"]},
                        {"table": "sensor_telemetry", "dataBindingType": "TimeSeries", "timestampColumn": "reading_ts", "properties": ["sensorId", "temperature", "reading_ts"]}
                    ]
                }
            },
            "relationships": {
                "monitors": {"table": "sensor", "sourceColumns": ["sensor_id"], "targetColumns": ["asset_id"]}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Create an empty ontology, then import the schema + bindings into it.
    let name = unique_name("ont_features");
    let created = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--file",
            rdf.to_str().unwrap(),
            "--lakehouse",
            &cfg.source_lakehouse,
            "--lakehouse-workspace",
            ws,
            "--bindings",
            map.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert();
    // Import may return a non-zero status only on real error; surface stderr.
    let out = res.get_output();
    assert!(
        out.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Read back the decoded definition and assert every new feature landed.
    let json = parse_json(
        &fabio()
            .args([
                "ontology",
                "get-definition",
                "--workspace",
                ws,
                "--id",
                &ont_id,
                "--decode",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    );
    let parts = decoded_parts(&json);

    // Sensor entity: inheritance + time-series + untyped.
    let sensor = part(&parts, "EntityTypes/8880000000002/definition.json");
    assert_eq!(sensor["name"], "Sensor");
    assert_eq!(
        sensor["baseEntityTypeId"], "8880000000001",
        "subClassOf -> baseEntityTypeId"
    );
    let ts: Vec<&str> = sensor["timeseriesProperties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        ts.contains(&"temperature") && ts.contains(&"reading_ts"),
        "timeseriesProperties: {ts:?}"
    );
    let untyped: Vec<&str> = sensor["untypedProperties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(untyped, vec!["payload"]);
    assert_eq!(sensor["untypedProperties"][0]["valueType"], "Any");

    // Two DataBindings for Sensor (static + telemetry).
    let sensor_dbs = parts
        .iter()
        .filter(|(p, _)| p.contains("EntityTypes/8880000000002/DataBindings"))
        .count();
    assert_eq!(sensor_dbs, 2, "expected 2 data bindings for Sensor");

    // Documents + relationship Contextualization present.
    assert!(
        parts
            .iter()
            .any(|(p, _)| p.contains("EntityTypes/8880000000002/Documents")),
        "Documents part"
    );
    assert!(
        parts
            .iter()
            .any(|(p, _)| p.contains("RelationshipTypes/9990000000001/Contextualizations")),
        "Contextualization part"
    );

    delete_ontology(ws, &ont_id);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn e2e_ontology_bind_and_schema_only_hint() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    std::fs::write(&rdf, FEATURES_RDF).unwrap();
    let map = dir.path().join("bindings.json");
    std::fs::write(
        &map,
        serde_json::to_string(&serde_json::json!({
            "entities": {
                "Sensor": {"table": "sensor", "timestampColumn": "reading_ts"},
                "Asset": {"table": "asset"}
            },
            "relationships": {"monitors": {"table": "sensor", "sourceColumns": ["sensor_id"], "targetColumns": ["asset_id"]}}
        }))
        .unwrap(),
    )
    .unwrap();

    let name = unique_name("ont_bind");
    let created = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Import types only (no data source) -> response carries the bind hint.
    let json = parse_json(
        &fabio()
            .args([
                "ontology",
                "import",
                "--workspace",
                ws,
                "--id",
                &ont_id,
                "--file",
                rdf.to_str().unwrap(),
            ])
            .timeout(std::time::Duration::from_mins(3))
            .assert()
            .success(),
    );
    assert!(
        extract_data(&json)["hint"]
            .as_str()
            .unwrap_or("")
            .contains("ontology bind"),
        "schema-only import should hint how to bind: {json}"
    );

    // Bind the live ontology's types to data (matches by name).
    fabio()
        .args([
            "ontology",
            "bind",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--lakehouse",
            &cfg.source_lakehouse,
            "--lakehouse-workspace",
            ws,
            "--bindings",
            map.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Bindings + contextualizations now exist.
    let json = parse_json(
        &fabio()
            .args([
                "ontology",
                "get-definition",
                "--workspace",
                ws,
                "--id",
                &ont_id,
                "--decode",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    );
    let parts = decoded_parts(&json);
    assert!(
        parts.iter().any(|(p, _)| p.contains("/DataBindings/")),
        "bind added DataBindings"
    );
    assert!(
        parts
            .iter()
            .any(|(p, _)| p.contains("/Contextualizations/")),
        "bind added Contextualizations"
    );

    delete_ontology(ws, &ont_id);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn e2e_ontology_import_lakehouse_schema_flag() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    // Simple non-time-series model so convention binding is a plain
    // NonTimeSeries LakehouseTable (no timestamp column needed).
    std::fs::write(
        &rdf,
        r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:ont="http://example.org/dt/">
  <owl:Class rdf:about="http://example.org/dt/Widget"><rdfs:label>Widget</rdfs:label></owl:Class>
  <owl:DatatypeProperty rdf:about="http://example.org/dt/widgetId">
    <rdfs:label>widgetId</rdfs:label>
    <rdfs:domain rdf:resource="http://example.org/dt/Widget"/>
    <rdfs:range rdf:resource="http://www.w3.org/2001/XMLSchema#string"/>
    <ont:isIdentifier rdf:datatype="http://www.w3.org/2001/XMLSchema#boolean">true</ont:isIdentifier>
  </owl:DatatypeProperty>
</rdf:RDF>"#,
    )
    .unwrap();

    let name = unique_name("ont_schemaflag");
    let created = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--file",
            rdf.to_str().unwrap(),
            "--lakehouse",
            &cfg.source_lakehouse,
            "--lakehouse-workspace",
            ws,
            "--lakehouse-schema",
            "silver",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(
        &fabio()
            .args([
                "ontology",
                "get-definition",
                "--workspace",
                ws,
                "--id",
                &ont_id,
                "--decode",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    );
    let parts = decoded_parts(&json);
    let db = part(&parts, "/DataBindings/");
    assert_eq!(
        db["dataBindingConfiguration"]["sourceTableProperties"]["sourceSchema"],
        "silver"
    );

    delete_ontology(ws, &ont_id);
}

/// Eventhouse (`KustoTable`) `TimeSeries` binding. Requires an existing Eventhouse
/// and KQL database; set these to run it, otherwise it no-ops:
///   `FABIO_TEST_EVENTHOUSE_ID`, `FABIO_TEST_EVENTHOUSE_CLUSTER_URI`,
///   `FABIO_TEST_EVENTHOUSE_DATABASE`
#[test]
#[ignore = "requires live Fabric tenant + Eventhouse"]
#[serial]
fn e2e_ontology_import_eventhouse_timeseries() {
    let (Ok(eh_id), Ok(cluster), Ok(db)) = (
        std::env::var("FABIO_TEST_EVENTHOUSE_ID"),
        std::env::var("FABIO_TEST_EVENTHOUSE_CLUSTER_URI"),
        std::env::var("FABIO_TEST_EVENTHOUSE_DATABASE"),
    ) else {
        eprintln!("[skip] set FABIO_TEST_EVENTHOUSE_ID/_CLUSTER_URI/_DATABASE to run");
        return;
    };
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    std::fs::write(&rdf, FEATURES_RDF).unwrap();

    let name = unique_name("ont_eh");
    let created = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Eventhouse default source: every entity binds TimeSeries to a KustoTable.
    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--file",
            rdf.to_str().unwrap(),
            "--eventhouse",
            &eh_id,
            "--eventhouse-workspace",
            ws,
            "--cluster-uri",
            &cluster,
            "--database",
            &db,
            "--timestamp-column",
            "reading_ts",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(
        &fabio()
            .args([
                "ontology",
                "get-definition",
                "--workspace",
                ws,
                "--id",
                &ont_id,
                "--decode",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    );
    let parts = decoded_parts(&json);
    let db_part = part(&parts, "/DataBindings/");
    assert_eq!(
        db_part["dataBindingConfiguration"]["dataBindingType"],
        "TimeSeries"
    );
    assert_eq!(
        db_part["dataBindingConfiguration"]["sourceTableProperties"]["sourceType"],
        "KustoTable"
    );
    assert_eq!(
        db_part["dataBindingConfiguration"]["sourceTableProperties"]["clusterUri"],
        cluster
    );

    delete_ontology(ws, &ont_id);
}

// ---------------------------------------------------------------------------
// MCP server URL
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_mcp_url_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("ont_mcp");

    // Create a bare ontology.
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let expected_url = format!(
        "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/{}/items/{}/ontologyEndpoint",
        cfg.source_workspace, ont_id
    );

    // Existing ontology: canonical URL, exists true, prerequisite note, no hint.
    let assert = fabio()
        .args([
            "ontology",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["mcpUrl"], expected_url);
    assert_eq!(data["transport"], "http");
    assert_eq!(data["exists"], true);
    assert!(data["note"].as_str().unwrap().contains("MCP server"));
    assert!(data["hint"].is_null());

    // Nonexistent ontology: still emits the deterministic URL, exists false + hint.
    let missing = "00000000-0000-0000-0000-000000000000";
    let assert = fabio()
        .args([
            "ontology",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            missing,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert!(
        data["mcpUrl"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/items/{missing}/ontologyEndpoint"))
    );
    assert_eq!(data["exists"], false);
    assert!(data["hint"].as_str().unwrap().contains("ontology list"));

    // Cleanup.
    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// list-entity-types (pure-fabio equivalent of the ontology MCP
// `list_ontology_entity_types` tool)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_list_entity_types_matches_mcp_shape() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Create an ontology and import a two-entity schema (Asset; Sensor inherits
    // Asset and has a timeseries property).
    let name = unique_name("ont_let");
    let assert = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    std::fs::write(&rdf, FEATURES_RDF).unwrap();
    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--file",
            rdf.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // list-entity-types --include-properties
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--include-properties",
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    let values = data["values"].as_array().expect("values array");
    assert_eq!(values.len(), 2, "Asset + Sensor");

    let by_name = |n: &str| {
        values
            .iter()
            .find(|v| v["name"] == n)
            .unwrap_or_else(|| panic!("entity {n} missing"))
            .clone()
    };
    let asset = by_name("Asset");
    let sensor = by_name("Sensor");

    // MCP-shape fields present; server-only etag and $schema must NOT leak.
    for v in [&asset, &sensor] {
        let obj = v.as_object().unwrap();
        for k in [
            "id",
            "namespace",
            "name",
            "namespaceType",
            "entityIdParts",
            "displayNamePropertyId",
            "visibility",
            "properties",
            "timeseriesProperties",
            "untypedProperties",
            "documents",
            "mappings",
            "resourceLinks",
        ] {
            assert!(obj.contains_key(k), "missing {k}");
        }
        assert!(!obj.contains_key("etag"), "etag must not leak");
        assert!(!obj.contains_key("$schema"), "$schema must not leak");
    }

    // Asset has no inheritance; Sensor inherits Asset and carries timeseries +
    // untyped properties, each reduced to {id,name,valueType} (no null fields).
    assert!(!asset.as_object().unwrap().contains_key("baseEntityTypeId"));
    assert_eq!(sensor["baseEntityTypeId"], asset["id"]);
    let ts_names: Vec<&str> = sensor["timeseriesProperties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(ts_names.contains(&"temperature"), "ts: {ts_names:?}");
    let temp = sensor["timeseriesProperties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "temperature")
        .unwrap();
    assert_eq!(temp["valueType"], "Double");
    assert!(!temp.as_object().unwrap().contains_key("redefines"));
    // payload is an untyped property.
    let untyped_names: Vec<&str> = sensor["untypedProperties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        untyped_names.contains(&"payload"),
        "untyped: {untyped_names:?}"
    );

    // --entity-name filter returns exactly that entity.
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity-name",
            "Sensor",
        ])
        .assert()
        .success();
    let filtered = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0]["name"], "Sensor");
    // Default (no --include-properties) empties the property arrays.
    assert_eq!(filtered[0]["properties"].as_array().unwrap().len(), 0);

    delete_ontology(ws, &ont_id);
}

// ---------------------------------------------------------------------------
// search (MCP client — consumes the ontology MCP `search_ontology` tool)
// ---------------------------------------------------------------------------

#[test]
fn ontology_search_dry_run_offline() {
    // Deterministic: --dry-run stops before any network/MCP call.
    let assert = fabio()
        .args([
            "ontology",
            "search",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--prompt",
            "How many assets are there?",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "ontology search");
    assert_eq!(data["details"]["tool"], "search_ontology");
    assert_eq!(data["details"]["query"], "How many assets are there?");
    // Endpoint must be the canonical ontology MCP URL.
    assert!(
        data["details"]["endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/ontologyEndpoint")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_search_drives_mcp_client() {
    // Validates fabio's MCP CLIENT end-to-end: create an ontology, then run
    // `ontology search`, which must connect to the ontology MCP server,
    // discover the search_ontology tool, call it, and return a structured
    // {query, answer, isError} response. A successful *answer* depends on
    // server-side Fabric IQ provisioning, so we assert the mechanism (the query
    // is echoed and an answer field is returned), not a specific answer.
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let name = unique_name("ont_search");

    let assert = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let dir = tempfile::tempdir().unwrap();
    let rdf = dir.path().join("dt.rdf");
    std::fs::write(&rdf, FEATURES_RDF).unwrap();
    fabio()
        .args([
            "ontology",
            "import",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--file",
            rdf.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Run search. On a tenant without Fabric IQ NL reasoning this returns
    // isError:true (non-zero exit), so do not assert success — inspect stdout.
    let output = fabio()
        .args([
            "ontology",
            "search",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--prompt",
            "How many assets are there?",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("search stdout not JSON: {stdout}"));
    let data = &json["data"];
    // The MCP round-trip happened: our query is echoed and an answer is present.
    assert_eq!(data["query"], "How many assets are there?");
    assert!(
        !data["answer"].is_null(),
        "expected an answer field: {json}"
    );
    assert!(
        data.get("isError").is_some(),
        "expected isError flag: {json}"
    );

    delete_ontology(ws, &ont_id);
}

// ---------------------------------------------------------------------------
// generate (client-side reproduction of the portal "Generate Ontology" from a
// semantic model — the tutorial's Part 1)
// ---------------------------------------------------------------------------

/// A minimal import-mode model.bim with two related tables (queryable via
/// INFO.VIEW without an external data source).
fn retail_model_bim() -> String {
    serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "tables": [
                {"name": "dimstore",
                 "columns": [
                    {"name": "StoreId", "dataType": "string", "sourceColumn": "StoreId"},
                    {"name": "City", "dataType": "string", "sourceColumn": "City"},
                    {"name": "Latitude", "dataType": "double", "sourceColumn": "Latitude"}],
                 "partitions": [{"name": "dimstore", "source": {"type": "m",
                    "expression": "let Source = #table(type table [StoreId=text,City=text,Latitude=number], {{\"S-PAR-01\",\"Paris\",48.85}}) in Source"}}]},
                {"name": "factsales",
                 "columns": [
                    {"name": "SaleId", "dataType": "int64", "sourceColumn": "SaleId"},
                    {"name": "StoreId", "dataType": "string", "sourceColumn": "StoreId"},
                    {"name": "RevenueUSD", "dataType": "double", "sourceColumn": "RevenueUSD"}],
                 "partitions": [{"name": "factsales", "source": {"type": "m",
                    "expression": "let Source = #table(type table [SaleId=Int64.Type,StoreId=text,RevenueUSD=number], {{1,\"S-PAR-01\",170.0}}) in Source"}}]}
            ],
            "relationships": [
                {"name": "r", "fromTable": "factsales", "fromColumn": "StoreId",
                 "toTable": "dimstore", "toColumn": "StoreId", "crossFilteringBehavior": "oneDirection"}
            ]
        }
    })
    .to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_generate_from_semantic_model() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Create a semantic model to generate from.
    let dir = tempfile::tempdir().unwrap();
    let bim = dir.path().join("model.bim");
    std::fs::write(&bim, retail_model_bim()).unwrap();
    let sm_name = unique_name("sm_gen");
    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            &sm_name,
            "--file",
            bim.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // generate --output-owl: read the model schema and synthesize OWL, no create.
    let owl_path = dir.path().join("gen.owl");
    let ont_name = unique_name("ont_gen");
    fabio()
        .args([
            "ontology",
            "generate",
            "--workspace",
            ws,
            "--semantic-model",
            &sm_id,
            "--name",
            &ont_name,
            "--output-owl",
            owl_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let owl = std::fs::read_to_string(&owl_path).unwrap();
    // Entity types = tables; typed properties; relationship = many->one side.
    assert!(owl.contains("<rdfs:label>dimstore</rdfs:label>"));
    assert!(owl.contains("<rdfs:label>factsales</rdfs:label>"));
    assert!(owl.contains("factsales_has_dimstore"));
    assert!(owl.contains("XMLSchema#double")); // RevenueUSD/Latitude
    assert!(owl.contains("XMLSchema#long")); // SaleId
    // dimstore.StoreId is the relationship's "one" side -> marked as identifier.
    let idx = owl.find("dimstore.StoreId").unwrap();
    assert!(owl[idx..idx + 400].contains("isIdentifier"));

    // Full generate: create the ontology and verify its entity types.
    let assert = fabio()
        .args([
            "ontology",
            "generate",
            "--workspace",
            ws,
            "--semantic-model",
            &sm_id,
            "--name",
            &ont_name,
            "--lakehouse",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let names: Vec<String> = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"dimstore".to_string()), "names: {names:?}");
    assert!(names.contains(&"factsales".to_string()), "names: {names:?}");

    // Cleanup.
    delete_ontology(ws, &ont_id);
    let _ = fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            ws,
            "--id",
            &sm_id,
        ])
        .assert();
}

/// Upload a CSV and load it as a Delta table into the given lakehouse.
fn load_csv_table(ws: &str, lh: &str, table: &str, csv: &str, dir: &std::path::Path) {
    let local = dir.join(format!("{table}.csv"));
    std::fs::write(&local, csv).unwrap();
    let remote = format!("Files/{table}.csv");
    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            ws,
            "--id",
            lh,
            "--source-path",
            local.to_str().unwrap(),
            "--dest-path",
            &remote,
        ])
        .assert()
        .success();
    fabio()
        .args([
            "lakehouse",
            "load-table",
            "--workspace",
            ws,
            "--id",
            lh,
            "--source-path",
            &remote,
            "--table",
            table,
            "--mode",
            "Overwrite",
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_generate_from_lakehouse() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let dir = tempfile::tempdir().unwrap();

    // Dedicated throwaway lakehouse so the generated ontology reflects exactly
    // the tables we load (and cleanup is a single delete).
    let lh_name = unique_name("lh_ontogen");
    let assert = fabio()
        .args(["lakehouse", "create", "--workspace", ws, "--name", &lh_name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let lh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Two tables: a string-keyed dimension and a numeric-keyed fact, exercising
    // the SQL->XSD type mapping and the first-column identifier heuristic.
    load_csv_table(
        ws,
        &lh_id,
        "dimstore",
        "StoreId,City,Latitude\nS-01,Paris,48.8566\nS-02,Lyon,45.7640\n",
        dir.path(),
    );
    load_csv_table(
        ws,
        &lh_id,
        "factsales",
        "SaleId,StoreId,Units,RevenueUSD\n1,S-01,34,170.0\n2,S-02,12,60.5\n",
        dir.path(),
    );

    // generate --output-owl: read the lakehouse schema (SQL endpoint) -> OWL.
    // The SQL analytics endpoint reflects newly-loaded Delta tables with a lag,
    // so poll until both tables surface (or time out).
    let owl_path = dir.path().join("gen_lh.owl");
    let ont_name = unique_name("ont_lhgen");
    let mut owl = String::new();
    for attempt in 0..12 {
        fabio()
            .args([
                "ontology",
                "generate",
                "--workspace",
                ws,
                "--lakehouse",
                &lh_id,
                "--name",
                &ont_name,
                "--output-owl",
                owl_path.to_str().unwrap(),
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success();
        owl = std::fs::read_to_string(&owl_path).unwrap();
        if owl.contains("<rdfs:label>dimstore</rdfs:label>")
            && owl.contains("<rdfs:label>factsales</rdfs:label>")
        {
            break;
        }
        assert!(attempt < 11, "SQL endpoint never surfaced tables:\n{owl}");
        std::thread::sleep(std::time::Duration::from_secs(20));
    }
    assert!(owl.contains("<rdfs:label>dimstore</rdfs:label>"));
    assert!(owl.contains("<rdfs:label>factsales</rdfs:label>"));
    // No relationships can be inferred from a lakehouse source.
    assert!(!owl.contains("<owl:ObjectProperty"));
    // Latitude/RevenueUSD -> xsd:double; SaleId/Units -> xsd:long.
    assert!(owl.contains("XMLSchema#double"));
    assert!(owl.contains("XMLSchema#long"));
    // First column of each table is the identifier heuristic.
    let idx = owl.find("dimstore.StoreId").unwrap();
    assert!(owl[idx..idx + 400].contains("isIdentifier"));
    let idx2 = owl.find("factsales.SaleId").unwrap();
    assert!(owl[idx2..idx2 + 400].contains("isIdentifier"));

    // Full generate: create the ontology + bind entity types to the lakehouse.
    let assert = fabio()
        .args([
            "ontology",
            "generate",
            "--workspace",
            ws,
            "--lakehouse",
            &lh_id,
            "--name",
            &ont_name,
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Entity types are created from the tables.
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let names: Vec<String> = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"dimstore".to_string()), "names: {names:?}");
    assert!(names.contains(&"factsales".to_string()), "names: {names:?}");

    // Data bindings to the lakehouse are emitted as DataBindings definition parts.
    let assert = fabio()
        .args([
            "ontology",
            "get-definition",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let has_binding = extract_data(&parse_json(&assert))["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| {
            p["path"]
                .as_str()
                .is_some_and(|s| s.contains("DataBindings/"))
        });
    assert!(
        has_binding,
        "expected DataBindings parts from --lakehouse bind"
    );

    // Cleanup.
    delete_ontology(ws, &ont_id);
    let _ = fabio()
        .args(["lakehouse", "delete", "--workspace", ws, "--id", &lh_id])
        .assert();
}

// ---------------------------------------------------------------------------
// Granular element editing (add/delete entity types, relationship types,
// report links) — client-side read-modify-write on the definition.
// ---------------------------------------------------------------------------

/// Self-seeding lifecycle for the granular ontology-editing commands: create an
/// empty ontology, add two entity types + a relationship + a report link,
/// verify via list-entity-types / list-report-links, then delete each element
/// (checking the entity-type delete cascades the relationship type).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ontology_granular_elements_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let name = unique_name("ont_granular");

    // Create an empty ontology.
    let assert = fabio()
        .args(["ontology", "create", "--workspace", ws, "--name", &name])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ont_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Add two entity types with typed properties + a key.
    let assert = fabio()
        .args([
            "ontology",
            "add-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--name",
            "Product",
            "--property",
            "ProductId:String",
            "--property",
            "Price:Double",
            "--key",
            "ProductId",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let d = parse_json(&assert);
    assert_eq!(extract_data(&d)["status"], "entity_type_added");
    assert_eq!(extract_data(&d)["keys"].as_array().unwrap().len(), 1);

    fabio()
        .args([
            "ontology",
            "add-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--name",
            "Store",
            "--property",
            "StoreId:String",
            "--key",
            "StoreId",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Duplicate entity type name is rejected.
    fabio()
        .args([
            "ontology",
            "add-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--name",
            "Product",
            "--property",
            "X:String",
        ])
        .assert()
        .failure();

    // Add a relationship type between them.
    let assert = fabio()
        .args([
            "ontology",
            "add-relationship-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--name",
            "soldAt",
            "--source",
            "Product",
            "--target",
            "Store",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["status"],
        "relationship_type_added"
    );

    // Verify both entity types are present.
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let names: Vec<String> = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str().map(str::to_string))
        .collect();
    assert!(names.contains(&"Product".to_string()) && names.contains(&"Store".to_string()));

    // Rename an entity type (Product -> ProductCatalog), then back. The
    // relationship references the stable entity id, so it must survive the rename.
    let assert = fabio()
        .args([
            "ontology",
            "rename-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "Product",
            "--new-name",
            "ProductCatalog",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["status"],
        "entity_type_renamed"
    );
    // The new name is present, the old is gone.
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let names: Vec<String> = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str().map(str::to_string))
        .collect();
    assert!(names.contains(&"ProductCatalog".to_string()));
    assert!(!names.contains(&"Product".to_string()));
    // Renaming to an existing name (Store) is rejected.
    fabio()
        .args([
            "ontology",
            "rename-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "ProductCatalog",
            "--new-name",
            "Store",
        ])
        .assert()
        .failure();
    // Rename back so the subsequent report-link steps still target "Product".
    fabio()
        .args([
            "ontology",
            "rename-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "ProductCatalog",
            "--new-name",
            "Product",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Add + list + delete a report link on Product.
    fabio()
        .args([
            "ontology",
            "add-report-link",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "Product",
            "--report-workspace",
            ws,
            "--report",
            "11111111-1111-4111-8111-111111111111",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let assert = fabio()
        .args([
            "ontology",
            "list-report-links",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    assert_eq!(extract_count(&parse_json(&assert)), 1);

    fabio()
        .args([
            "ontology",
            "delete-report-link",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "Product",
            "--report",
            "11111111-1111-4111-8111-111111111111",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // delete-entity-type cascades the relationship type referencing it.
    let assert = fabio()
        .args([
            "ontology",
            "delete-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "Store",
            "--dry-run",
        ])
        .assert()
        .success();
    let cascaded = parse_json(&assert);
    assert_eq!(cascaded["data"]["dry_run"], true);
    assert_eq!(
        cascaded["data"]["details"]["cascadedRelationshipTypes"][0],
        "soldAt"
    );

    fabio()
        .args([
            "ontology",
            "delete-entity-type",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--entity",
            "Store",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Only Product remains; the relationship type is gone.
    let assert = fabio()
        .args([
            "ontology",
            "list-entity-types",
            "--workspace",
            ws,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
    let remaining: Vec<String> = extract_data(&parse_json(&assert))["values"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(remaining, vec!["Product".to_string()]);

    delete_ontology(ws, &ont_id);
}
