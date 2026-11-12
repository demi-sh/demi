//! The registry: every plugin the composition root registers, in that
//! order, checked once when the backend starts, then each plugin a program
//! declares while it runs, checked as it is declared. What the startup
//! catalog does not serve is left out: a command tree, a user stream or a
//! page method bound to an operation the catalog lacks.
//!
//! A registry never changes: a program's declaration or removal of one of
//! its plugins makes the next ([`Plugins`]). A plugin keeps its index in
//! every later registry, so a command set or a request that names a plugin
//! by its index names it whatever was declared since. A removed plugin
//! keeps its index, and no answer of the registry includes it.

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError, RwLock};

use demi_command_declarations::{LeafKind, NativeOperation, Node};
use demi_device_interface::RegisterError;
use demi_plugin_interface::{
    Commands, DEMI_ROOT, Manifest, Page, Placement, PluginFactory, PluginFile, PluginId, Scope, State, Stream,
};

use crate::commands::{TAKEN_GROUPS, compose};
use crate::features::{self, FeatureError};

/// Why the backend cannot start with its plugins; the error names the
/// plugin.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("two plugins have the id \"{0}\"")]
    DuplicateId(PluginId),
    #[error("plugin \"{plugin}\" declares \"{command}\", which is taken")]
    Taken { plugin: PluginId, command: String },
    #[error("plugin \"{plugin}\" declares the user stream \"{name}\", which is taken")]
    Stream { plugin: PluginId, name: String },
    #[error("plugin \"{plugin}\"'s page package \"{package}\" is another plugin's")]
    PagePackage { plugin: PluginId, package: String },
    #[error("plugin \"{plugin}\" declares the tab kind \"{kind}\", which is taken")]
    TabKind { plugin: PluginId, kind: String },
    #[error("plugin \"{plugin}\"'s commands are refused: {error}")]
    Commands {
        plugin: PluginId,
        error: RegisterError,
    },
    #[error("plugin \"{plugin}\"'s OAuth sign-in names {issuer}; an issuer is an https URL without a query")]
    OAuthIssuer { plugin: PluginId, issuer: String },
    #[error("plugin \"{plugin}\"'s OAuth sign-in names no vendor")]
    OAuthVendor { plugin: PluginId },
    #[error("plugin \"{plugin}\"'s features are refused: {error}")]
    Features { plugin: PluginId, error: FeatureError },
}

/// One registered plugin, and what of its manifest the catalog serves.
#[derive(Clone)]
pub(crate) struct Registered {
    pub(crate) factory: Arc<dyn PluginFactory>,
    pub(crate) commands: Vec<Commands>,
    pub(crate) streams: Vec<Stream>,
    pub(crate) page: Option<Page>,
    /// The packages its served commands, streams and methods bind: the
    /// only ones its package calls may name.
    pub(crate) packages: BTreeSet<String>,
    /// The number of its program's declaration of it, which a toolset's
    /// revision names; none for a plugin linked into the program.
    pub(crate) declaration: Option<u64>,
    /// Its program removed it.
    pub(crate) removed: bool,
}

impl Registered {
    pub(crate) fn id(&self) -> &PluginId {
        &self.factory.manifest().id
    }

    /// Its page's state of `scope`, if it declares one.
    pub(crate) fn state(&self, scope: Scope) -> Option<&State> {
        self.page.as_ref().and_then(|page| page.state(scope))
    }

    /// Whether it declares the tab kind `kind`.
    pub(crate) fn owns_kind(&self, kind: &str) -> bool {
        self.page
            .as_ref()
            .is_some_and(|page| page.tab_kinds.iter().any(|owned| owned == kind))
    }
}

/// The program's plugins as they are now: the registry it started with,
/// which each declaration and removal of one of its plugins replaces. A
/// core reads the newest, and adopts each next one
/// ([`crate::UserPlugins::adopt`]).
pub struct Plugins {
    current: RwLock<Arc<Registry>>,
    serves: Box<dyn Fn(&NativeOperation) -> bool + Send + Sync>,
}

impl Plugins {
    /// The registry of `factories` ([`Registry::new`]), whose later
    /// declarations `serves` checks as it checked these.
    pub fn new(
        factories: Vec<Box<dyn PluginFactory>>,
        serves: impl Fn(&NativeOperation) -> bool + Send + Sync + 'static,
    ) -> Result<Self, RegistryError> {
        let registry = Registry::new(factories, &serves)?;
        Ok(Self {
            current: RwLock::new(Arc::new(registry)),
            serves: Box::new(serves),
        })
    }

    /// The registry now.
    pub fn current(&self) -> Arc<Registry> {
        self.current.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Declares `factory`, a plugin of the program's, in place of the one
    /// of its id or after the others; the manifest declared already leaves
    /// the registry as it is. A manifest that breaks a rule, or the id of a
    /// plugin linked into the program, is refused.
    pub fn declare(&self, factory: Arc<dyn PluginFactory>) -> Result<(), RegistryError> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(next) = current.with_declared(factory, &self.serves)? {
            *current = Arc::new(next);
        }
        Ok(())
    }

    /// Removes the program's plugin `id`: answers its id, or none for an id
    /// no program declared.
    pub fn remove(&self, id: &str) -> Option<PluginId> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        let removed = current.declared(id)?;
        let next = current.without(id)?;
        *current = Arc::new(next);
        Some(removed)
    }
}

/// The program's plugins at one moment, shared by every shard thread.
pub struct Registry {
    /// Every plugin it ever registered, by its index; a removed one
    /// included.
    pub(crate) plugins: Vec<Registered>,
    /// The number of the last declaration.
    declarations: u64,
}

impl Registry {
    /// The plugins that are context sources, in registration order, which
    /// a node asks before each provider request while its user has them on.
    pub fn context_sources(&self) -> Vec<PluginId> {
        self.present()
            .filter(|(_, registered)| registered.factory.manifest().context)
            .map(|(_, registered)| registered.id().clone())
            .collect()
    }

    /// The registry of `factories`, in their order. What binds an operation
    /// `serves` refuses is left out, and logged; a manifest that breaks a
    /// rule stops the start.
    fn new(factories: Vec<Box<dyn PluginFactory>>, serves: &impl Fn(&NativeOperation) -> bool) -> Result<Self, RegistryError> {
        let mut plugins = Vec::new();
        for factory in factories {
            let factory: Arc<dyn PluginFactory> = Arc::from(factory);
            check_manifest(factory.manifest())?;
            plugins.push(served(factory, serves, None));
        }
        Self::checked(plugins, 0)
    }

    /// The id of `id`, a plugin a program declared, while it is present.
    pub fn declared(&self, id: &str) -> Option<PluginId> {
        self.plugin(id)
            .filter(|(_, registered)| registered.declaration.is_some())
            .map(|(_, registered)| registered.id().clone())
    }

    /// This registry with the program's plugin `factory` in place of the one
    /// of its id, or after the others; none when that manifest is declared
    /// already.
    fn with_declared(
        &self,
        factory: Arc<dyn PluginFactory>,
        serves: &impl Fn(&NativeOperation) -> bool,
    ) -> Result<Option<Self>, RegistryError> {
        let manifest = factory.manifest();
        check_manifest(manifest)?;
        let declaration = self.declarations + 1;
        let mut plugins = self.plugins.clone();
        match plugins.iter().position(|registered| registered.id() == &manifest.id) {
            Some(index) => {
                let held = &plugins[index];
                if held.declaration.is_none() {
                    return Err(RegistryError::DuplicateId(manifest.id.clone()));
                }
                if !held.removed && held.factory.manifest() == manifest {
                    return Ok(None);
                }
                plugins[index] = served(factory, serves, Some(declaration));
            }
            None => plugins.push(served(factory, serves, Some(declaration))),
        }
        Self::checked(plugins, declaration).map(Some)
    }

    /// This registry without the program's plugin `id`; none when no
    /// plugin of the program's has that id.
    fn without(&self, id: &str) -> Option<Self> {
        let (index, registered) = self.plugin(id)?;
        registered.declaration?;
        let mut plugins = self.plugins.clone();
        plugins[index].removed = true;
        Some(Self {
            plugins,
            declarations: self.declarations,
        })
    }

    /// The registry of `plugins`, which no two of claim the same name of.
    fn checked(plugins: Vec<Registered>, declarations: u64) -> Result<Self, RegistryError> {
        let registry = Self { plugins, declarations };
        let mut ids = BTreeSet::new();
        let mut stream_names = BTreeSet::new();
        let mut page_packages = BTreeSet::new();
        let mut tab_kinds = BTreeSet::new();
        for registered in &registry.plugins {
            let manifest = registered.factory.manifest();
            // A removed plugin keeps its id, which a declaration takes again.
            if !ids.insert(manifest.id.clone()) {
                return Err(RegistryError::DuplicateId(manifest.id.clone()));
            }
            if registered.removed {
                continue;
            }
            for stream in &manifest.streams {
                if !stream_names.insert(stream.name.clone()) {
                    return Err(RegistryError::Stream {
                        plugin: manifest.id.clone(),
                        name: stream.name.clone(),
                    });
                }
            }
            if let Some(page) = &manifest.page {
                if !page_packages.insert(page.package.clone()) {
                    return Err(RegistryError::PagePackage {
                        plugin: manifest.id.clone(),
                        package: page.package.clone(),
                    });
                }
                for kind in &page.tab_kinds {
                    if !tab_kinds.insert(kind.clone()) {
                        return Err(RegistryError::TabKind {
                            plugin: manifest.id.clone(),
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }
        registry.check()?;
        Ok(registry)
    }

    /// The plugins it has, by their indices: none removed.
    pub(crate) fn present(&self) -> impl Iterator<Item = (usize, &Registered)> {
        self.plugins
            .iter()
            .enumerate()
            .filter(|(_, registered)| !registered.removed)
    }

    /// Composes the command set once, with no instance behind it, so a name
    /// taken twice is found now and never while a conversation runs. The
    /// `demi` root is the plugin host's, so a root of that name is taken
    /// whichever plugin comes first.
    fn check(&self) -> Result<(), RegistryError> {
        let mut demi = BTreeSet::new();
        for (_, registered) in self.present() {
            for commands in &registered.commands {
                let name = commands.tree.name();
                let (taken, command) = match commands.placement {
                    Placement::Demi => (
                        TAKEN_GROUPS.contains(&name) || !demi.insert(name.to_owned()),
                        format!("{DEMI_ROOT} {name}"),
                    ),
                    Placement::Root => (name == DEMI_ROOT, name.to_owned()),
                };
                if taken {
                    return Err(RegistryError::Taken {
                        plugin: registered.id().clone(),
                        command,
                    });
                }
            }
        }
        compose(self, None, Vec::new(), |_| true, |_, _| true).map(|_| ())
    }

    /// The plugin that declares the user stream `name`, by its index.
    pub(crate) fn stream_owner(&self, name: &str) -> Option<usize> {
        self.present()
            .find(|(_, registered)| registered.streams.iter().any(|stream| stream.name == name))
            .map(|(index, _)| index)
    }

    /// Every user stream the catalog serves, of every plugin.
    pub fn streams(&self) -> impl Iterator<Item = &Stream> {
        self.present().flat_map(|(_, registered)| &registered.streams)
    }

    /// The plugin that declares the tab kind `kind`, by its index.
    pub(crate) fn kind_owner(&self, kind: &str) -> Option<usize> {
        self.present()
            .find(|(_, registered)| registered.owns_kind(kind))
            .map(|(index, _)| index)
    }

    /// The file `name` the plugin `plugin`'s pages load
    /// ([`PluginFactory::file`]), the same for every user.
    pub fn file(&self, plugin: &str, name: &str) -> Option<PluginFile> {
        self.plugin(plugin).and_then(|(_, registered)| registered.factory.file(name))
    }

    pub(crate) fn plugin(&self, id: &str) -> Option<(usize, &Registered)> {
        self.present().find(|(_, registered)| registered.id().as_str() == id)
    }
}

/// Refuses a manifest that breaks a rule of its own.
fn check_manifest(manifest: &Manifest) -> Result<(), RegistryError> {
    if let Some(oauth) = &manifest.oauth {
        // RFC 8414 § 2: an https URL with no query or fragment.
        let issuer = &oauth.issuer;
        if issuer.scheme() != "https" || issuer.query().is_some() || issuer.fragment().is_some() {
            return Err(RegistryError::OAuthIssuer {
                plugin: manifest.id.clone(),
                issuer: issuer.to_string(),
            });
        }
        if oauth.vendor.trim().is_empty() {
            return Err(RegistryError::OAuthVendor {
                plugin: manifest.id.clone(),
            });
        }
    }
    features::check(&manifest.commands, &manifest.features).map_err(|error| RegistryError::Features {
        plugin: manifest.id.clone(),
        error,
    })
}

/// `factory` with what of its manifest `serves` serves.
fn served(
    factory: Arc<dyn PluginFactory>,
    serves: &impl Fn(&NativeOperation) -> bool,
    declaration: Option<u64>,
) -> Registered {
    let manifest: &Manifest = factory.manifest();
    let left_out = |what: &str, name: &str| {
        tracing::info!(
            event = "plugin_part.left_out",
            plugin = %manifest.id,
            what,
            name,
            reason = "not_served",
            "a part whose package the catalog does not serve is left out"
        );
    };
    let mut packages = BTreeSet::new();
    let mut bound = |operations: &[&NativeOperation]| {
        packages.extend(operations.iter().map(|operation| operation.package.clone()));
    };
    let mut commands = Vec::new();
    for tree in &manifest.commands {
        let mut operations = native_operations(&tree.tree);
        operations.extend(&tree.calls);
        if operations.iter().all(|operation| serves(operation)) {
            bound(&operations);
            commands.push(tree.clone());
        } else {
            left_out("command", tree.tree.name());
        }
    }
    let mut streams = Vec::new();
    for stream in &manifest.streams {
        if serves(&stream.operation) {
            bound(&[&stream.operation]);
            streams.push(stream.clone());
        } else {
            left_out("user stream", &stream.name);
        }
    }
    let page = manifest.page.as_ref().map(|page| {
        let mut page = page.clone();
        for (scope, state) in [
            ("user", &mut page.user),
            ("conversation", &mut page.conversation),
        ] {
            let kept = state.as_ref().is_none_or(|state| {
                let operations: Vec<&NativeOperation> = state.operations.iter().collect();
                let kept = operations.iter().all(|operation| serves(operation));
                if kept {
                    bound(&operations);
                }
                kept
            });
            if !kept {
                left_out("page state", scope);
                *state = None;
            }
        }
        page.methods.retain(|method| {
            let operations: Vec<&NativeOperation> = method.operations.iter().collect();
            let kept = operations.iter().all(|operation| serves(operation));
            if kept {
                bound(&operations);
            } else {
                left_out("page method", &method.name);
            }
            kept
        });
        page
    });
    Registered {
        commands,
        streams,
        page,
        packages,
        factory,
        declaration,
        removed: false,
    }
}

/// Every native operation a tree's leaves bind.
fn native_operations(tree: &Node<NativeOperation>) -> Vec<&NativeOperation> {
    match tree {
        Node::Leaf(leaf) => match &leaf.kind {
            LeafKind::Native(operation) => vec![operation],
            LeafKind::Rpc => Vec::new(),
        },
        Node::Group(group) => group
            .subcommands
            .iter()
            .flat_map(native_operations)
            .collect(),
    }
}
