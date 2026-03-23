use crate::commands::Handler;
use crate::commands::generic::GenericHandler;
use crate::config::Config;

pub struct PackageMgrHandler;

impl Handler for PackageMgrHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        GenericHandler.compress(cmd, lines, config)
    }
}
