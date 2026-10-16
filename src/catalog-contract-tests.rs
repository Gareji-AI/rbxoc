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
