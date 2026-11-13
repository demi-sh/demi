//! The built-in plugins, one per plugin crate, in their order of registration.
//!
//! This is the list the composition root registers. A plugin leaves a
//! deployment by leaving this list.

use std::path::Path;

use jush_plugin_audio::Audio;
use jush_plugin_browser::Browser;
use jush_plugin_changes::Changes;
use jush_plugin_file::File;
use jush_plugin_file_browser::FileBrowser;
use jush_plugin_image::Image;
use jush_plugin_video::Video;
use jush_plugin_interface::PluginFactory;
use jush_plugin_mobile::Mobile;
use jush_plugin_skills::Skills;
use jush_plugin_web::Web;
use jush_plugin_widget::Widgets;

/// The plugins built into the backend whose data is under `data_dir`.
pub fn builtin(data_dir: &Path) -> Vec<Box<dyn PluginFactory>> {
    vec![
        Box::new(File::new()),
        Box::new(Browser::new()),
        Box::new(Mobile::new()),
        Box::new(Skills::new(data_dir.join("tmp/skill-fetches"))),
        Box::new(Web::new()),
        Box::new(Image::new()),
        Box::new(Video::new()),
        Box::new(Audio::new()),
        Box::new(Widgets::new()),
        Box::new(Changes::new()),
        Box::new(FileBrowser::new()),
    ]
}
