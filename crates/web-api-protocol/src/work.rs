//! A conversation's work: its tabs, and the layout of the windows they open in,
//! which the backend keeps and changes a request of changes at a time, each request
//! counted by the work's revision. The backend checks their shape and bounds and never
//! interprets a tab's `kind` or `data`, or the layout.

use jush_shared_types::MAX_SAFE_INTEGER;
use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The most tabs one work holds.
///
/// Bounded because the work's document is stored for each conversation and
/// sent whole at each change.
pub const WORK_TABS_MAX: usize = 64;

/// The most bytes of one work's tabs, as the backend stores them.
///
/// Bounded for the same reason as [`WORK_TABS_MAX`].
pub const WORK_BYTES_MAX: usize = 64 * 1024;

/// The most bytes of one work's window layout, as the backend stores it.
///
/// Bounded for the same reason as [`WORK_TABS_MAX`]; a layout of every
/// window the page has, each with its place, its box and where it stood when
/// it closed, is under 2 KiB.
pub const WORK_LAYOUT_BYTES_MAX: usize = 16 * 1024;

/// A conversation's work as the database keeps it: the tabs in order, and
/// how many changes made them. The conversation's entry in the list carries
/// it, so the page draws the work's frame and tabs as soon as the
/// conversation is listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct Work {
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub revision: u64,
    #[garde(dive)]
    pub tabs: Vec<WorkTab>,
    /// Where the conversation's windows stand, as the page last kept it;
    /// absent while it kept none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub windows: Option<Map<String, Value>>,
}

impl Work {
    /// The work of a conversation that never changed it.
    pub fn empty() -> Self {
        Self {
            revision: 0,
            tabs: Vec::new(),
            windows: None,
        }
    }
}

/// A plugin's conversation state as the plugin holds it, with the revision it
/// was held at (`PluginStateAnswer`). A conversation's metadata carries the
/// states of the plugins that declare theirs held, so a kind's content has
/// them when the page reads it, never a read of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct HeldState {
    #[garde(length(min = 1))]
    pub plugin: String,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub revision: u64,
    #[garde(skip)]
    pub state: Value,
}

/// One tab of the work: its kind, and what the page and the kind's plugin
/// keep for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct WorkTab {
    #[garde(length(min = 1))]
    pub id: String,
    #[garde(length(min = 1))]
    pub kind: String,
    #[garde(skip)]
    pub data: Map<String, Value>,
}

/// A new tab at `index` among the work's tabs, after the others without one.
/// The tabs of one window stand in its strip in this order, so `index` places
/// the tab among its window's tabs; tabs of other windows between them change
/// nothing there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkTab {
    #[garde(length(min = 1))]
    pub id: String,
    #[garde(length(min = 1))]
    pub kind: String,
    #[garde(skip)]
    pub data: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(range(max = WORK_TABS_MAX))]
    pub index: Option<usize>,
}

/// `POST /conversations/:id/work/changes`: changes applied in order, all
/// or none, as one change of the work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct WorkChanges {
    #[garde(length(min = 1), dive)]
    pub changes: Vec<WorkChange>,
}

/// What a request of changes answers: the work's revision once the
/// changes are in it, and whether the request changed the work, which a
/// revision alone cannot tell when another change came in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct WorkAnswer {
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub revision: u64,
    #[garde(skip)]
    pub changed: bool,
}

/// One change of a conversation's work, from the page or from the kind's plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkChange {
    /// Creates the tab.
    Create(#[garde(dive)] CreateWorkTab),
    /// Sets each field of `data` in the tab's `data`, removing those that
    /// are null, and leaves the other fields.
    Update {
        #[garde(length(min = 1))]
        id: String,
        #[garde(skip)]
        data: Map<String, Value>,
    },
    /// Removes the tab.
    #[serde(rename = "delete")]
    Remove {
        #[garde(length(min = 1))]
        id: String,
    },
    /// Moves the tab to `index` among the others.
    Move {
        #[garde(length(min = 1))]
        id: String,
        #[garde(range(max = WORK_TABS_MAX))]
        index: usize,
    },
    /// Replaces where the windows stand.
    Windows {
        #[garde(skip)]
        windows: Map<String, Value>,
    },
}

/// What the backend keeps of a conversation's work: its tabs in order, and every id it
/// had, which is never used again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct WorkDocument {
    #[garde(dive)]
    pub tabs: Vec<WorkTab>,
    #[garde(skip)]
    pub retired: Vec<String>,
    /// Where the windows stand; absent in a document that never had a layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub windows: Option<Map<String, Value>>,
}

/// What a change did to which tab.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkEffect {
    Created(WorkTab),
    Updated,
    /// The tab as it was when it was removed.
    Removed(WorkTab),
    Moved,
    /// The windows' layout was replaced.
    Arranged,
}

/// What applying a change came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    Effect(WorkEffect),
    /// Nothing to do: a tab the work no longer has, an id it has or had,
    /// or a change that changes nothing.
    Nothing,
    /// A create past the most tabs or the most data.
    Full,
    /// An update or a layout past the most data.
    TooLarge,
}

/// Why a request of changes was refused, whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// A create past the most tabs or the most data.
    Full,
    /// An update or a layout past the most data.
    TooLarge,
}

/// What making a kind's tabs a device's list came to
/// ([`WorkDocument::mirror`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Mirrored {
    pub effects: Vec<WorkEffect>,
    /// The listed ids whose tabs the user closed: the device did not hear
    /// of it, and closes them now.
    pub closed: Vec<String>,
}

/// Why a change was not applied, before the tabs' size is known.
enum Unapplied {
    Nothing,
    Full,
    TooLarge,
}

impl WorkDocument {
    /// Applies `changes` in order, all or none: each that has nothing to do
    /// is passed over, and one that does not fit leaves the document as it
    /// was and answers why.
    pub fn apply_all(&mut self, changes: Vec<WorkChange>) -> Result<Vec<WorkEffect>, Refused> {
        let mut next = self.clone();
        let mut effects = Vec::new();
        for change in changes {
            match next.apply(change) {
                Applied::Effect(effect) => effects.push(effect),
                Applied::Nothing => {}
                Applied::Full => return Err(Refused::Full),
                Applied::TooLarge => return Err(Refused::TooLarge),
            }
        }
        *self = next;
        Ok(effects)
    }

    /// Makes the tabs of `kind` the tabs `listed`, the tabs a device has
    /// open, as one change: creates each listed tab the document never had,
    /// at its index; sets the data of each it has to the listed data; and
    /// removes each tab of the kind the list does not name. A listed id the
    /// document had and retired is a tab its user closed, answered in
    /// [`Mirrored::closed`].
    ///
    /// A tab that does not fit, past the most tabs or the most data, is left
    /// out, or keeps the data it had, while every other changes: one tab too
    /// many never holds the others back. A later list places it once there
    /// is room.
    pub fn mirror(&mut self, kind: &str, listed: Vec<CreateWorkTab>) -> Mirrored {
        let named: std::collections::HashSet<String> = listed.iter().map(|tab| tab.id.clone()).collect();
        let mut changes = Vec::new();
        let mut closed = Vec::new();
        // Created and set first: a listed tab's index counts the tabs as
        // they stand before the unlisted ones go.
        for create in listed {
            if self.retired.contains(&create.id) {
                closed.push(create.id);
                continue;
            }
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == create.id) else {
                changes.push(WorkChange::Create(create));
                continue;
            };
            let mut data: Map<String, Value> = create
                .data
                .iter()
                .filter(|(field, value)| tab.data.get(*field) != Some(*value))
                .map(|(field, value)| (field.clone(), value.clone()))
                .collect();
            data.extend(
                tab.data
                    .keys()
                    .filter(|field| !create.data.contains_key(*field))
                    .map(|field| (field.clone(), Value::Null)),
            );
            if !data.is_empty() {
                changes.push(WorkChange::Update { id: create.id, data });
            }
        }
        let removals: Vec<WorkChange> = self
            .tabs
            .iter()
            .filter(|tab| tab.kind == kind && !named.contains(&tab.id))
            .map(|tab| WorkChange::Remove { id: tab.id.clone() })
            .collect();
        let mut effects = Vec::new();
        // A change that does not fit beside the tabs that go may fit once
        // they went.
        let mut waiting = Vec::new();
        for change in changes {
            match self.apply(change.clone()) {
                Applied::Effect(effect) => effects.push(effect),
                Applied::Full | Applied::TooLarge => waiting.push(change),
                Applied::Nothing => {}
            }
        }
        for change in removals.into_iter().chain(waiting) {
            if let Applied::Effect(effect) = self.apply(change) {
                effects.push(effect);
            }
        }
        Mirrored { effects, closed }
    }

    /// Applies `change`, or leaves the document as it was when the change
    /// has nothing to do or does not fit.
    pub fn apply(&mut self, change: WorkChange) -> Applied {
        let mut next = self.clone();
        let effect = match next.change(change) {
            Ok(effect) => effect,
            Err(Unapplied::Nothing) => return Applied::Nothing,
            Err(Unapplied::Full) => return Applied::Full,
            Err(Unapplied::TooLarge) => return Applied::TooLarge,
        };
        // The tabs serialize as JSON strings, arrays and objects with string
        // keys, which serde_json never refuses.
        let bytes = serde_json::to_string(&next.tabs)
            .expect("the work's tabs serialize to JSON")
            .len();
        if bytes > WORK_BYTES_MAX {
            return match effect {
                WorkEffect::Created(_) => Applied::Full,
                _ => Applied::TooLarge,
            };
        }
        *self = next;
        Applied::Effect(effect)
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn change(&mut self, change: WorkChange) -> Result<WorkEffect, Unapplied> {
        match change {
            WorkChange::Create(create) => {
                if self.position(&create.id).is_some() || self.retired.contains(&create.id) {
                    return Err(Unapplied::Nothing);
                }
                if self.tabs.len() >= WORK_TABS_MAX {
                    return Err(Unapplied::Full);
                }
                let tab = WorkTab {
                    id: create.id,
                    kind: create.kind,
                    data: create.data,
                };
                let index = create.index.unwrap_or(self.tabs.len()).min(self.tabs.len());
                self.tabs.insert(index, tab.clone());
                Ok(WorkEffect::Created(tab))
            }
            WorkChange::Update { id, data } => {
                let index = self.position(&id).ok_or(Unapplied::Nothing)?;
                let tab = &mut self.tabs[index];
                let before = tab.data.clone();
                for (field, value) in data {
                    if value == Value::Null {
                        tab.data.remove(&field);
                    } else {
                        tab.data.insert(field, value);
                    }
                }
                if tab.data == before {
                    return Err(Unapplied::Nothing);
                }
                Ok(WorkEffect::Updated)
            }
            WorkChange::Remove { id } => {
                let index = self.position(&id).ok_or(Unapplied::Nothing)?;
                let tab = self.tabs.remove(index);
                self.retired.push(tab.id.clone());
                Ok(WorkEffect::Removed(tab))
            }
            WorkChange::Move { id, index } => {
                let from = self.position(&id).ok_or(Unapplied::Nothing)?;
                let to = index.min(self.tabs.len() - 1);
                if to == from {
                    return Err(Unapplied::Nothing);
                }
                let tab = self.tabs.remove(from);
                self.tabs.insert(to, tab);
                Ok(WorkEffect::Moved)
            }
            WorkChange::Windows { windows } => {
                if self.windows.as_ref() == Some(&windows) {
                    return Err(Unapplied::Nothing);
                }
                // A JSON object serializes whole, as the tabs do.
                let bytes = serde_json::to_string(&windows)
                    .expect("a JSON object serializes")
                    .len();
                if bytes > WORK_LAYOUT_BYTES_MAX {
                    return Err(Unapplied::TooLarge);
                }
                self.windows = Some(windows);
                Ok(WorkEffect::Arranged)
            }
        }
    }
}
