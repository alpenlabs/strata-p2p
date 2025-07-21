//! Filtering class : interface for Banlist, Allowlist and future scoring.

use std::collections::HashSet;

use libp2p::identity::PublicKey;

/// Filtering: an interface for allowlist,banlist and maybe in future scoring
pub trait Filtering: std::fmt::Debug + Send + Sync + Clone + 'static {
    /// Adds an application's public key to the list of respected keys,
    /// allowing connections from this application.
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey);

    /// Removes an application's public key from the list of respected keys,
    /// disallowing connections from this application.
    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey);

    /// Checks if a given application's public key is currently respected,
    /// indicating whether a connection from it is allowed.
    ///
    /// Returns `true` if the application's public key is respected, `false` otherwise.
    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool;
}

/// Allow list aka Whitelist.
#[derive(Debug, Clone)]
pub struct AllowList {
    /// Set of app_pk in allow list.
    app_pk_allowlist: HashSet<PublicKey>,
}

impl AllowList {
    /// Create filtering "Allowlist" with predefined allow list.
    pub const fn new(pre_defined_allow_list: HashSet<PublicKey>) -> Self {
        AllowList {
            app_pk_allowlist: pre_defined_allow_list,
        }
    }
}

impl Filtering for AllowList {
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey) {
        self.app_pk_allowlist.insert(app_pk);
    }

    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey) {
        self.app_pk_allowlist.remove(&app_pk);
    }

    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool {
        self.app_pk_allowlist.contains(app_pk)
    }
}

/// Ban list aka blacklist.
#[derive(Debug, Clone)]
pub struct BanList {
    /// Set of app_pk in ban list.
    app_pk_banlist: HashSet<PublicKey>,
}

impl BanList {
    /// Create filtering "Banlist" with predefined ban list.
    pub const fn new(pre_defined_ban_list: HashSet<PublicKey>) -> Self {
        BanList {
            app_pk_banlist: pre_defined_ban_list,
        }
    }
}

impl Filtering for BanList {
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey) {
        // To respect an app_pk in a banlist, remove it from the banlist.
        self.app_pk_banlist.remove(&app_pk);
    }

    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey) {
        // To disrespect an app_pk in a banlist, add it to the banlist.
        self.app_pk_banlist.insert(app_pk);
    }

    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool {
        // An app_pk is respected enough to connect if it's NOT in the banlist.
        !self.app_pk_banlist.contains(app_pk)
    }
}
