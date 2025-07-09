//! Events emitted from P2P.

<<<<<<< HEAD
/// Events emitted from P2P to handle from operator side.
///
/// # Implementation Details
///
/// `DepositSetupPayload` is generic as some implementation details for different BitVM2
/// applications may vary. The only requirement for them to be decodable from bytes as protobuf
/// message.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant)]
pub enum Event {
    /// Received message from other operator.
    ReceivedMessage(Vec<u8>),

    /// Received a request from other operator.
=======
/// Events emitted from P2P that needs to be handled by the user.

#[derive(Debug, Clone)]
pub enum Event {
    /// Received message from other peer.
    ReceivedMessage(Vec<u8>),

    /// Received a request from other peer.
>>>>>>> dev-v2
    ReceivedRequest(Vec<u8>),
}
