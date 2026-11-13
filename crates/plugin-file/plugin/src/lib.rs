//! The `file` plugin: the `jush file`
//! group, every leaf bound to an operation of the `jush.file` command
//! package, which runs it on the device.

mod file;

use std::rc::Rc;

use jush_plugin_interface::{CommandPlugin, Manifest, Placement, Plugin, PluginFactory, PluginId};

/// The plugin's factory.
pub struct File {
    manifest: Manifest,
}

impl File {
    pub fn new() -> Self {
        let mut manifest = Manifest::new(
            PluginId::try_from("file").expect("a valid plugin id"),
            "Jush File Commands",
            "Reads, writes, edits and searches the conversation's files with jush file.",
        );
        manifest.commands = commands().manifest_commands();
        Self { manifest }
    }
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginFactory for File {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Its leaves are all native, so no request reaches the instance.
    fn instance(&self) -> Rc<dyn Plugin> {
        Rc::new(commands())
    }
}

/// The plugin's `jush file` group.
fn commands() -> CommandPlugin {
    CommandPlugin::new(Placement::Jush, vec![file::file_group()])
        .expect("the file group is a valid declaration")
}
