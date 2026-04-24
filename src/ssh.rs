use crate::keychain::{retrieve_from_keychain, store_in_keychain};
use crate::ssh_config::{self, parser_error::ParseError, HostVecExt};
use anyhow::anyhow;
use handlebars::Handlebars;
use itertools::Itertools;
use serde::Serialize;
use std::clone::Clone;
use std::collections::VecDeque;
use std::io;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use strum_macros::Display;

#[derive(Debug, Serialize, Clone)]
pub struct Host {
    pub name: String,
    pub aliases: String,
    pub description: String,
    pub user: Option<String>,
    pub destination: String,
    pub password: Option<String>,
    pub port: Option<String>,
    pub proxy_command: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct SshConnection {
    name: String,
    password: String,
}

fn ask_user(prompt: &str) -> io::Result<String> {
    print!("{}", prompt); // выводим приглашение без перевода строки
    io::stdout().flush()?; // немедленно выводим приглашение на экран

    let mut input = String::new();
    io::stdin().read_line(&mut input)?; // читаем строку (включая \n)

    Ok(input.to_string()) // убираем \n и пробелы по краям
}

fn option_str() -> anyhow::Result<Option<String>> {
    return Ok(None);
}

#[derive(PartialEq, Display)]
enum Connection {
    Ok,
    Nope,
}
impl Host {
    pub fn run_command_template_safe(&self, pattern: &str) -> anyhow::Result<()> {
        let rendered_command;

        let handlebars = Handlebars::new();
        rendered_command = handlebars.render_template(pattern, &self)?;

        let mut args = shlex::split(&rendered_command)
            .ok_or(anyhow!("Failed to parse command: {rendered_command}"))?
            .into_iter()
            .collect::<VecDeque<String>>();
        let command = args.pop_front().ok_or(anyhow!("Failed to get command"))?;

        let status = Command::new(command).args(args)
            .stdout(Stdio::null()) 
            .stderr(Stdio::null()) 
            .spawn()?.wait()?;
        
        if !status.success() {
            return Err(anyhow!("Error"));
        }

        return Ok(());
    }
    /// Uses the provided Handlebars template to run a command.
    ///
    /// # Errors
    ///
    /// Will return `Err` if the command cannot be executed.
    ///
    /// # Panics
    ///
    /// Will panic if the regex cannot be compiled.
    pub fn run_command_template(&self, pattern: &str) -> anyhow::Result<()> {
        let rendered_command;

        let handlebars = Handlebars::new();
        rendered_command = handlebars.render_template(pattern, &self)?;

        println!("Running command: {rendered_command}");
        let mut args = shlex::split(&rendered_command)
            .ok_or(anyhow!("Failed to parse command: {rendered_command}"))?
            .into_iter()
            .collect::<VecDeque<String>>();
        let command = args.pop_front().ok_or(anyhow!("Failed to get command"))?;

        let status = Command::new(command).args(args).spawn()?.wait()?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }

        return Ok(());
    }

    pub fn run_connect_command_template(&self) -> anyhow::Result<()> {
        let rendered_command;

        let pattern_check_pass =
            "ssh -o BatchMode=yes \"{{{name}}}\" \"exit\" 2>/dev/null && echo true || exit 1";

        let pattern_password = "sshpass -p \"{{{password}}}\" ssh \"{{{name}}}\"";
        let pattern_name = "ssh \"{{{name}}}\"";

        let need_passoword_to_connect = self.run_command_template_safe(pattern_check_pass);

        let passord: Option<String> = if need_passoword_to_connect.is_err() {
            if self.password.is_some() {
                Some("-1".to_string())
            } else {
                match retrieve_from_keychain(self.name.as_str()) {
                    Ok(pwd) => Some(pwd.to_string()),
                    Err(_) => {
                        let pwd_for_store = ask_user("Enter password to store in keychain: ")?;
                        if let Err(e) = store_in_keychain(self.name.as_str(), &pwd_for_store) {
                            eprintln!("Warning: failed to save password to keychain: {}", e);
                            return Err(anyhow!("Failed to store password: {}", e));
                        }
                        Some(pwd_for_store)
                    }
                }
            }
        } else {
            None
        };

        let alias = self.name.to_string();

        match passord {
            None => {
                let handlebars = Handlebars::new();
                rendered_command = handlebars.render_template(
                    pattern_name,
                    &self,
                )?;
            }
            Some(pwd) => {
                let handlebars = Handlebars::new();
                
                if pwd == "-1" {
                    rendered_command = handlebars.render_template(
                        pattern_password,
                        &self
                    )?
                } else {
                    rendered_command = handlebars.render_template(
                        pattern_password,
                        &SshConnection {
                            name: alias,
                            password: pwd,
                        },
                    )?
                };
            }
        };

        // println!("Running command: {rendered_command}");
        let mut args = shlex::split(&rendered_command)
            .ok_or(anyhow!("Failed to parse command: {rendered_command}"))?
            .into_iter()
            .collect::<VecDeque<String>>();
        let command = args.pop_front().ok_or(anyhow!("Failed to get command"))?;

        let status = Command::new(command).args(args).spawn()?.wait()?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum ParseConfigError {
    Io(std::io::Error),
    SshConfig(ParseError),
}

impl From<std::io::Error> for ParseConfigError {
    fn from(e: std::io::Error) -> Self {
        ParseConfigError::Io(e)
    }
}

impl From<ParseError> for ParseConfigError {
    fn from(e: ParseError) -> Self {
        ParseConfigError::SshConfig(e)
    }
}

/// # Errors
///
/// Will return `Err` if the SSH configuration file cannot be parsed.
pub fn parse_config(raw_path: &String) -> Result<Vec<Host>, ParseConfigError> {
    let normalized_path = shellexpand::tilde(&raw_path).to_string();
    let path = std::fs::canonicalize(normalized_path)?;

    let hosts = ssh_config::Parser::new()
        .parse_file(path)?
        .apply_patterns()
        .apply_name_to_empty_hostname()
        .merge_same_hosts()
        .iter()
        .map(|host| Host {
            name: host
                .get_patterns()
                .first()
                .unwrap_or(&String::new())
                .clone(),
            aliases: host.get_patterns().iter().skip(1).join(", "),
            description: host
                .get(&ssh_config::EntryType::Description)
                .unwrap_or_default(),
            user: host.get(&ssh_config::EntryType::User),
            password: host.get(&ssh_config::EntryType::Password),
            destination: host
                .get(&ssh_config::EntryType::Hostname)
                .unwrap_or_default(),
            port: host.get(&ssh_config::EntryType::Port),
            proxy_command: host.get(&ssh_config::EntryType::ProxyCommand),
        })
        .collect();

    Ok(hosts)
}
