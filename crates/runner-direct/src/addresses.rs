//! The addresses a runner offers a page (`crates/runner-direct/src/lib.rs`): `127.0.0.1`, and the IPv4 addresses of its interfaces that are
//! up, can broadcast and are not a tunnel. The fake address a VPN client or a
//! proxy's TUN mode gives is never offered: WebKit's encryption handshake
//! times out over one, and a path through it is relayed by the client's own
//! process, so a pair of runners on one network reached each other at a
//! fraction of its speed.
//!
//! A tunnel is told by its interface's type, not its name, which each client
//! chooses: on macOS and Linux it is point-to-point; on Windows, where Wintun,
//! WireGuard's and TAP-Windows' adapters are not, its type is also the
//! proprietary virtual one. An adapter of the Ethernet type stays offered,
//! a virtual one such as Hyper-V's included: on a host whose external switch
//! it is, it holds the host's address on the local network.

use std::net::Ipv4Addr;

use if_addrs::{IfAddr, Interface};

/// Where a runner's peers bind their sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Addresses {
    /// `127.0.0.1` and the qualifying addresses of the interfaces as they
    /// are when a peer is made.
    Interfaces,
    /// These addresses only, as a test binds.
    Only(Vec<Ipv4Addr>),
}

impl Addresses {
    /// The addresses to bind a new peer's sockets on.
    pub(crate) fn current(&self) -> Vec<Ipv4Addr> {
        match self {
            Self::Only(addresses) => addresses.clone(),
            Self::Interfaces => {
                let interfaces = match if_addrs::get_if_addrs() {
                    Ok(interfaces) => interfaces,
                    Err(error) => {
                        // Loopback alone still serves a browser on this
                        // machine.
                        tracing::warn!(event = "network_interfaces.list_failed", %error, "the network interfaces could not be listed");
                        Vec::new()
                    }
                };
                offered(&interfaces)
            }
        }
    }
}

/// `127.0.0.1`, then each IPv4 address of `interfaces` that is up, can
/// broadcast and is not a tunnel (`tunnel`), each once.
pub fn offered(interfaces: &[Interface]) -> Vec<Ipv4Addr> {
    let mut addresses = vec![Ipv4Addr::LOCALHOST];
    for interface in interfaces {
        let IfAddr::V4(address) = &interface.addr else {
            continue;
        };
        let qualifies = interface.is_oper_up()
            && address.broadcast.is_some()
            && !address.ip.is_loopback()
            && !tunnel(interface);
        if qualifies && !addresses.contains(&address.ip) {
            addresses.push(address.ip);
        }
    }
    addresses
}

/// Whether `interface` is a tunnel: point-to-point.
#[cfg(not(windows))]
fn tunnel(interface: &Interface) -> bool {
    interface.is_p2p()
}

/// Whether `interface` is a tunnel: point-to-point, or of the proprietary
/// virtual type. On Windows this asks the system for the interface's type,
/// so it reads the machine as well as `interface`. An interface whose type
/// cannot be read is offered: a path through it is slower at worst.
#[cfg(windows)]
fn tunnel(interface: &Interface) -> bool {
    use windows_sys::Win32::NetworkManagement::IpHelper::IF_TYPE_PROP_VIRTUAL;
    if interface.is_p2p() {
        return true;
    }
    let Some(index) = interface.index else {
        return false;
    };
    match interface_type(index) {
        Ok(kind) => kind == IF_TYPE_PROP_VIRTUAL,
        Err(status) => {
            tracing::debug!(event = "network_interface.type_failed", index, status, "a network interface's type could not be read");
            false
        }
    }
}

/// The type of the interface with `index`, as `ipifcons.h` numbers them, or
/// the status `GetIfEntry2` failed with.
#[cfg(windows)]
fn interface_type(index: u32) -> Result<u32, u32> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetIfEntry2, MIB_IF_ROW2};
    // Its LUID left zero, `GetIfEntry2` finds the interface by its index.
    let mut row = MIB_IF_ROW2 { InterfaceIndex: index, ..Default::default() };
    // SAFETY: `row` is a live, writable row for the call's duration.
    let status = unsafe { GetIfEntry2(&mut row) };
    if status == ERROR_SUCCESS { Ok(row.Type) } else { Err(status) }
}
