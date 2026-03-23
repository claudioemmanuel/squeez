use crate::commands::Handler;
use crate::commands::generic::GenericHandler;
use crate::config::Config;

pub struct DatabaseHandler;

impl Handler for DatabaseHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        GenericHandler.compress(cmd, lines, config)
    }
}
