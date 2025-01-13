pub enum PeerDepositState {
    /// State before sent setup for depostit.
    PreSetup,
    /// Peer sent setup for deposit.
    Setup,
    /// Peer sent nonces for deposit.
    Nonces,
    /// Peer sent signatures for deposit.
    Sigs,
}
