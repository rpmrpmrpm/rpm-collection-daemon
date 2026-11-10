//! Who may read. The caller decides; this module only defines the question.

use rpm_wire::PeerKind;

/// The trust check, asked on every request so that a key taken off the list stops working at once.
pub trait Trust: Send + Sync {
    /// How the owner's computer knows this key, or `None` when it does not trust it.
    fn kind_of(&self, public_key: &[u8]) -> Option<PeerKind>;

    /// Whether this key belongs to an owner of the library: a computer that may upload the catalogue
    /// and the trust list.
    fn is_owner(&self, public_key: &[u8]) -> bool;
}

/// A reader that has proved its key, as the trust check sees it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    pub public_key: Vec<u8>,
    pub kind: PeerKind,
    pub owner: bool,
}

/// The reader for `public_key`, or `None` when it is neither trusted nor an owner. An owner always
/// reads as one of the owner's own devices.
pub fn standing(trust: &dyn Trust, public_key: &[u8]) -> Option<Reader> {
    let owner = trust.is_owner(public_key);
    let kind = match (trust.kind_of(public_key), owner) {
        (_, true) => PeerKind::Account,
        (Some(kind), false) => kind,
        (None, false) => return None,
    };
    Some(Reader {
        public_key: public_key.to_vec(),
        kind,
        owner,
    })
}
