//! Discover actual clap leaves, not a hand-maintained endpoint count.

use crate::{catalog, cli::Cli};
use clap::CommandFactory;
use std::collections::BTreeSet;

fn leaves(command: &clap::Command, prefix: &str, out: &mut Vec<String>) {
    for sub in command
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help")
    {
        let path = format!("{prefix}{}", sub.get_name());
        if sub.get_subcommands().next().is_none() {
            out.push(path);
        } else {
            leaves(sub, &format!("{path} "), out);
        }
    }
}

fn contract_errors(entries: &[catalog::Endpoint]) -> Vec<String> {
    let mut commands = Vec::new();
    leaves(&Cli::command(), "", &mut commands);
    let mut covered = BTreeSet::new();
    let mut errors = Vec::new();
    for command in commands {
        // Local composites do not correspond to individual cloud endpoints.
        if command.starts_with("api ")
            || command.starts_with("assets ")
            || command == "analytics catalog"
        {
            continue;
        }
        let id = if command == "universe publish-message" {
            "messaging.publish".into()
        } else {
            command
                .replace("analytics alerts ", "analytics alert ")
                .replace(' ', ".")
        };
        let Some(endpoint) = entries.iter().find(|entry| entry.id == id) else {
            errors.push(format!("{command} has no catalog entry ({id})"));
            continue;
        };
        covered.insert(endpoint.id);
        if !endpoint.implemented {
            errors.push(format!("{command} is incorrectly marked unimplemented"));
        }
        let action = command.split_whitespace().last().unwrap();
        let read_only = [
            "check",
            "get",
            "list",
            "status",
            "history",
            "logs",
            "operation",
            "versions",
            "restart-forecast",
            "metrics",
            "draft",
            "revisions",
            "incidents",
            "image-id",
        ]
        .contains(&action);
        if endpoint.mutating == read_only {
            errors.push(format!(
                "{command} has an incorrect mutation classification"
            ));
        }
    }
    for endpoint in entries
        .iter()
        .filter(|entry| entry.implemented && entry.method != "LOCAL")
    {
        if !covered.contains(endpoint.id) {
            errors.push(format!("{} has no CLI endpoint command", endpoint.id));
        }
    }
    errors
}

#[test]
fn command_catalog_contract_covers_every_cloud_leaf_in_both_directions() {
    let errors = contract_errors(catalog::ENDPOINTS);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn command_catalog_contract_detects_drift_even_when_counts_match() {
    let mut entries = catalog::ENDPOINTS.to_vec();
    entries.retain(|entry| entry.id != "devproduct.create");
    let mut replacement = catalog::ENDPOINTS[0].clone();
    replacement.id = "nonexistent.command";
    entries.push(replacement);
    let errors = contract_errors(&entries);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("devproduct create"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("nonexistent.command"))
    );
}

/// Independent security/scope excerpts from the official OpenAPI snapshot.
/// The fixture retains the provider fields rather than copying catalog values.
fn auth_contract_errors(entries: &[catalog::Endpoint]) -> Vec<String> {
    let snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/openapi-auth-contract.json")).unwrap();
    let mut errors = Vec::new();
    let mut covered = BTreeSet::new();
    for operation in snapshot["operations"].as_array().unwrap() {
        let id = operation["catalogId"].as_str().unwrap();
        covered.insert(id);
        let Some(entry) = entries.iter().find(|entry| entry.id == id) else {
            errors.push(format!("{id}: catalog entry missing"));
            continue;
        };
        let security = operation["security"].as_array().unwrap();
        let mut auth = BTreeSet::new();
        let mut scopes = BTreeSet::new();
        for alternative in security {
            if alternative.get("roblox-api-key").is_some() {
                auth.insert("api-key");
            }
            if let Some(oauth) = alternative.get("roblox-oauth2") {
                auth.insert("oauth");
                scopes.extend(
                    oauth
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap()),
                );
            }
            // Cookie alternatives and an empty anonymous alternative are not
            // credential families implemented by this authenticated CLI.
        }
        scopes.extend(
            operation["x-roblox-scopes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|scope| scope["name"].as_str().unwrap()),
        );
        let actual_auth: BTreeSet<_> = entry
            .auth
            .iter()
            .map(|kind| match kind {
                catalog::AuthKind::ApiKey => "api-key",
                catalog::AuthKind::OAuth => "oauth",
                catalog::AuthKind::LegacyCookie => "legacy-cookie",
            })
            .collect();
        if actual_auth != auth {
            errors.push(format!("{id}: auth {actual_auth:?} != OpenAPI {auth:?}"));
        }
        let actual_scopes: BTreeSet<_> = entry.scopes.iter().copied().collect();
        if actual_scopes != scopes {
            errors.push(format!(
                "{id}: scopes {actual_scopes:?} != OpenAPI {scopes:?}"
            ));
        }
        if entry.method != operation["method"].as_str().unwrap() {
            errors.push(format!("{id}: method differs from snapshot"));
        }
        let actual_path = entry
            .path
            .replace("/resource/Universe/", "/resource/{resourceType}/");
        let expected_path = operation["path"]
            .as_str()
            .unwrap()
            .replace("/operations/metrics/", "/operations/{kind}/")
            .replace("/operations/dimension-values/", "/operations/{kind}/");
        if path_shape(&actual_path) != path_shape(&expected_path) {
            errors.push(format!("{id}: route differs from official operation"));
        }
    }
    for entry in entries
        .iter()
        .filter(|entry| entry.implemented && entry.method != "LOCAL")
    {
        if !covered.contains(entry.id) {
            errors.push(format!("{}: no official auth/permission fixture", entry.id));
        }
    }
    errors
}

fn path_shape(path: &str) -> String {
    let mut output = String::new();
    let mut parameter = false;
    for c in path.chars() {
        match c {
            '{' => {
                parameter = true;
                output.push_str("{}");
            }
            '}' => parameter = false,
            _ if !parameter => output.push(c),
            _ => (),
        }
    }
    output
}

#[test]
fn catalog_auth_and_permissions_match_official_operations() {
    let errors = auth_contract_errors(catalog::ENDPOINTS);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// #54: `implemented: true` must mean "this exact method and route is in the
/// current official document". A route that silently disappears from the
/// provider contract must fail here instead of staying advertised.
#[test]
fn every_implemented_route_exists_in_the_current_official_snapshot() {
    let snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/openapi-auth-contract.json")).unwrap();
    let mut routes = BTreeSet::new();
    for operation in snapshot["operations"].as_array().unwrap() {
        routes.insert(segments(operation["path"].as_str().unwrap()));
    }
    let mut errors = Vec::new();
    for entry in catalog::ENDPOINTS
        .iter()
        .filter(|entry| entry.implemented && entry.method != "LOCAL")
    {
        let wanted = segments(entry.path);
        // A catalog parameter may stand for a documented literal when the
        // provider lists several variants: `operations/{kind}/{id}` covers
        // both `operations/metrics/{id}` and `operations/dimension-values/{id}`.
        // Literal segments must still match, so a removed or renamed route
        // cannot be hidden.
        let matched = routes.iter().any(|route| {
            route.len() == wanted.len()
                && route
                    .iter()
                    .zip(&wanted)
                    .all(|(actual, wanted)| *wanted == "*" || actual == wanted)
        });
        if !matched {
            errors.push(format!(
                "{}: {} {} is not in the current official OpenAPI snapshot",
                entry.id, entry.method, entry.path
            ));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// Split a path into segments, replacing every `{parameter}` with `*`.
fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "*".to_string()
            } else {
                segment.to_string()
            }
        })
        .collect()
}

#[test]
fn a_route_removed_from_the_official_contract_is_detected() {
    // Remove the alert family from the snapshot the way the provider would.
    let mut snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/openapi-auth-contract.json")).unwrap();
    let operations = snapshot["operations"].as_array_mut().unwrap();
    operations.retain(|operation| {
        !operation["path"]
            .as_str()
            .unwrap()
            .contains("analytics-alert-control-plane")
    });
    let remaining: Vec<Vec<String>> = operations
        .iter()
        .map(|operation| segments(operation["path"].as_str().unwrap()))
        .collect();
    let advertised = catalog::ENDPOINTS
        .iter()
        .filter(|entry| entry.id.starts_with("analytics.alert."))
        .filter(|entry| {
            let wanted = segments(entry.path);
            !remaining.iter().any(|route| {
                route.len() == wanted.len()
                    && route
                        .iter()
                        .zip(&wanted)
                        .all(|(actual, wanted)| *wanted == "*" || actual == wanted)
            })
        })
        .count();
    assert_eq!(
        advertised, 5,
        "every alert operation must be detected as gone"
    );
}

#[test]
fn analytics_catalog_note_does_not_contradict_supported_credentials() {
    let entry = catalog::ENDPOINTS
        .iter()
        .find(|e| e.id == "analytics.metrics")
        .unwrap();
    assert!(entry.auth.contains(&catalog::AuthKind::ApiKey));
    assert!(!entry.note.contains("not an API key"), "{}", entry.note);
    assert!(entry.note.contains("API keys and OAuth2"), "{}", entry.note);
}

#[test]
fn auth_contract_detects_credential_and_scope_drift() {
    let mut entries = catalog::ENDPOINTS.to_vec();
    let endpoint = entries
        .iter_mut()
        .find(|e| e.id == "analytics.metrics")
        .unwrap();
    endpoint.auth = &[catalog::AuthKind::LegacyCookie];
    endpoint.scopes = &["invented:permission"];
    let errors = auth_contract_errors(&entries);
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("analytics.metrics: auth"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("analytics.metrics: scopes"))
    );
}
