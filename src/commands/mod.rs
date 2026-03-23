use crate::config::Config;

pub trait Handler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String>;
}

pub mod build;
pub mod cloud;
pub mod database;
pub mod docker;
pub mod filter_stdin;
pub mod fs;
pub mod generic;
pub mod git;
pub mod init;
pub mod network;
pub mod package_mgr;
pub mod runtime;
pub mod test_runner;
pub mod track;
pub mod typescript;
pub mod wrap;
