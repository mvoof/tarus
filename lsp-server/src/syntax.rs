// src/syntax.rs
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "camelCase")]
pub enum EntityType {
    Command,
    Event,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Behavior {
    Definition, // fn my_command
    Call,       // invoke('my_command')
    Emit,       // emit('event')
    Listen,     // listen('event')
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ArgSource {
    /// The name is taken from the argument (e.g. 0 or 1)
    Index { index: usize },
    /// The name is taken from the name of the function itself
    FunctionName,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub name: String,
    pub entity: EntityType,
    pub behavior: Behavior,
    pub args: ArgSource,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BackendSyntax {
    pub attributes: Vec<Rule>,
    pub functions: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FrontendSyntax {
    pub functions: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommandSyntax {
    pub frontend: FrontendSyntax,
    pub backend: BackendSyntax,
}

pub fn load_syntax<P: AsRef<Path>>(
    config_path: P,
) -> Result<CommandSyntax, Box<dyn Error + Send + Sync>> {
    let content = fs::read_to_string(config_path)?;
    let syntax = serde_json::from_str(&content)?;

    Ok(syntax)
}
