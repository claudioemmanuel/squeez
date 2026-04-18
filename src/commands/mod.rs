use crate::config::Config;

pub trait Handler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String>;
}

pub mod build;
pub mod cloud;
pub mod compress_md;
pub mod data_tool;
pub mod database;
pub mod docker;
pub mod filter_stdin;
pub mod fs;
pub mod generic;
pub mod git;
pub mod init;
pub mod mcp_server;
pub mod network;
pub mod next_build;
pub mod persona;
pub mod package_mgr;
pub mod playwright;
pub mod protocol;
pub mod runtime;
pub mod test_runner;
pub mod wrangler;
pub mod text_proc;
pub mod track;
pub mod track_result;
pub mod typescript;
pub mod benchmark;
pub mod setup;
pub mod update;
pub mod wrap;
