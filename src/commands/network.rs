use crate::commands::Handler;
use crate::commands::generic::GenericHandler;
use crate::config::Config;

pub struct NetworkHandler;

impl Handler for NetworkHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        GenericHandler.compress(cmd, lines, config)
    }
}
