use crate::commands::Handler;
use crate::commands::generic::GenericHandler;
use crate::config::Config;

pub struct CloudHandler;

impl Handler for CloudHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        GenericHandler.compress(cmd, lines, config)
    }
}
