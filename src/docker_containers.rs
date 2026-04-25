use anyhow::{Context, Result};
use clap::Parser;
use serde::de::Unexpected;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::Error;
use std::process::{Command, Stdio};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::{Modifier, Style, Text};
use ratatui::style::palette::tailwind;
use ratatui::widgets::{Block, BorderType, Borders, Cell, HighlightSpacing, Row, Table};
use crate::ssh;
use crate::ui::App;

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



pub fn render_docker_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_style = Style::default().fg(tailwind::CYAN.c500);
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);

    let mut header_names = vec![
        "Names",
        "Ports",
        "Image",
        "CreatedAt",
        "RunningFor",
        "Size",
        "State",
        "Status",
    ];

    let header = header_names
        .iter()
        .copied()
        .map(Cell::from)
        .collect::<Row>()
        .style(header_style)
        .height(1);

    let bar = " █ ";
    let v: &Vec<Container> = &Vec::new();

    if app.hosts.len() == 0 {
        return;
    }

    let selected = app.table_state_hosts.selected().unwrap_or(0);
    let host: ssh::Host = app.hosts[selected].clone();
    let containers = &app.docker_containers;

    let rows = containers.get(&host).unwrap_or(v).iter().map(|cont| {
        let mut content = vec![
            cont.name.clone(),
            cont.ports.clone(),
            cont.image.clone(),
            cont.created_at.clone(),
            cont.running_for.clone(),
            cont.size.clone(),
            cont.state.clone(),
            cont.status.clone(),
        ];

        content
            .iter()
            .map(|content| Cell::from(Text::from(content.to_string())))
            .collect::<Row>()
    });

    let t = Table::new(rows, app.table_columns_constraints.clone())
        .header(header)
        .row_highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![
            "".into(),
            bar.into(),
            bar.into(),
            "".into(),
        ]))
        .highlight_spacing(HighlightSpacing::Always)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(app.palette.c400))
                .border_type(BorderType::Rounded),
        );
    f.render_stateful_widget(t, area, &mut app.table_state_containers);
}
