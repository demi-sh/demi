//! What a viewer's page receives: the live view's writer
//! (`jush_live_view::writer`), carrying the browser's messages.

use jush_command_package_browser_protocol::live::LiveModuleMessage;

pub(crate) type Writer = jush_live_view::writer::Writer<LiveModuleMessage>;
