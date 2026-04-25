use anyhow::{Context, Result};
use clap::Parser;
use serde::de::Unexpected;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::Error;
use std::process::{Command, Stdio};

#[derive(Parser, Debug, Deserialize, Serialize)]

pub struct Container {

    #[serde(rename = "Names")]
    pub name: String,
    #[serde(rename = "Ports")]
    pub ports: String,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "CreatedAt")]
    pub created_at: String,
    #[serde(rename = "RunningFor")]
    pub running_for: String,
    #[serde(rename = "Size")]
    pub size: String,
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Status")]
    pub status: String,
}


pub fn resolve_json_docker_containers(output: String) -> anyhow::Result<Vec<Container>> {
    let mut containers = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Container>(line) {
            Ok(c) => containers.push(c),
            Err(e) => {}
        }
    }
    Ok(containers)
}
