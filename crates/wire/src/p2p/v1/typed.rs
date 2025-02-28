//! Types derived from the `.proto` files.

use bitcoin::{
    consensus::{Decodable, ReadExt},
    hashes::{sha256, Hash},
    io::{Cursor, Read},
    OutPoint, Txid, XOnlyPublicKey,
};
use musig2::{PartialSignature, PubNonce};
use prost::{DecodeError, Message};
use strata_p2p_types::{
    OperatorPubKey, Scope, SessionId, StakeChainId, StakeData, Wots256PublicKey, WotsPublicKeys,
    WOTS_SINGLE,
};

use super::proto::{
    get_message_request::Body as ProtoGetMessageRequestBody,
    gossipsub_msg::Body as ProtoGossipsubMsgBody, DepositRequestKey,
    DepositSetupExchange as ProtoDepositSetup, GetMessageRequest as ProtoGetMessageRequest,
    GossipsubMsg as ProtoGossipMsg, Musig2NoncesExchange as ProtoMusig2NoncesExchange,
    Musig2RequestKey, Musig2SignaturesExchange as ProtoMusig2SignaturesExchange,
    StakeChainExchange as ProtoStakeChainExchange, StakeChainRequestKey,
};

/// Typed version of "get_message_request::GetMessageRequest".
#[derive(Clone, Debug)]
pub enum GetMessageRequest {
    /// Request Stake Chain info for this operator.
    StakeChainExchange {
        stake_chain_id: StakeChainId,
        operator_pk: OperatorPubKey,
    },

    /// Request deposit setup info for [`Scope`] and operator.
    ///
    /// This is primarily used for the WOTS PKs.
    DepositSetup {
        scope: Scope,
        operator_pk: OperatorPubKey,
    },

    /// Request MuSig2 (partial) signatures from operator and for [`SessionId`].
    Musig2SignaturesExchange {
        session_id: SessionId,
        operator_pk: OperatorPubKey,
    },

    /// Request MuSig2 (public) nonces from operator and for [`SessionId`].
    Musig2NoncesExchange {
        session_id: SessionId,
        operator_pk: OperatorPubKey,
    },
}

impl GetMessageRequest {
    /// Converts [`ProtoGetMessageRequest`] into [`GetMessageRequest`]
    /// by parsing raw vec values into specific types.
    pub fn from_msg(msg: ProtoGetMessageRequest) -> Result<GetMessageRequest, DecodeError> {
        let body = msg.body.ok_or(DecodeError::new("Message without body"))?;

        let request = match body {
            ProtoGetMessageRequestBody::DepositSetup(DepositRequestKey { scope, operator }) => {
                let bytes = scope
                    .try_into()
                    .map_err(|_| DecodeError::new("invalid length of bytes in scope"))?;
                let scope = Scope::from_bytes(bytes);

                Self::DepositSetup {
                    scope,
                    operator_pk: operator.into(),
                }
            }
            ProtoGetMessageRequestBody::Nonces(Musig2RequestKey {
                session_id,
                operator,
            }) => {
                let bytes = session_id
                    .try_into()
                    .map_err(|_| DecodeError::new("invalid length of bytes in session id"))?;
                let session_id = SessionId::from_bytes(bytes);

                Self::Musig2NoncesExchange {
                    session_id,
                    operator_pk: operator.into(),
                }
            }
            ProtoGetMessageRequestBody::Sigs(Musig2RequestKey {
                session_id,
                operator,
            }) => {
                let bytes = session_id
                    .try_into()
                    .map_err(|_| DecodeError::new("invalid length of bytes in session id"))?;
                let session_id = SessionId::from_bytes(bytes);

                Self::Musig2SignaturesExchange {
                    session_id,
                    operator_pk: operator.into(),
                }
            }
            ProtoGetMessageRequestBody::StakeChain(StakeChainRequestKey {
                stake_chain_id,
                operator,
            }) => {
                let bytes = stake_chain_id
                    .try_into()
                    .map_err(|_| DecodeError::new("invalid length of bytes in stake chain id"))?;
                let stake_chain_id = StakeChainId::from_bytes(bytes);
                Self::StakeChainExchange {
                    stake_chain_id,
                    operator_pk: operator.into(),
                }
            }
        };

        Ok(request)
    }

    /// Converts [`GetMessageRequest`] into raw [`ProtoGetMessageRequest`].
    pub fn into_msg(self) -> ProtoGetMessageRequest {
        let body = match self {
            Self::StakeChainExchange {
                stake_chain_id,
                operator_pk,
            } => ProtoGetMessageRequestBody::StakeChain(StakeChainRequestKey {
                stake_chain_id: stake_chain_id.to_vec(),
                operator: operator_pk.into(),
            }),
            Self::DepositSetup { scope, operator_pk } => {
                ProtoGetMessageRequestBody::DepositSetup(DepositRequestKey {
                    scope: scope.to_vec(),
                    operator: operator_pk.into(),
                })
            }
            Self::Musig2SignaturesExchange {
                session_id,
                operator_pk,
            } => ProtoGetMessageRequestBody::Nonces(Musig2RequestKey {
                session_id: session_id.to_vec(),
                operator: operator_pk.into(),
            }),
            Self::Musig2NoncesExchange {
                session_id,
                operator_pk,
            } => ProtoGetMessageRequestBody::Sigs(Musig2RequestKey {
                session_id: session_id.to_vec(),
                operator: operator_pk.into(),
            }),
        };

        ProtoGetMessageRequest { body: Some(body) }
    }

    /// Returns the [`OperatorPubKey`] with respect to this [`GetMessageRequest`].
    pub fn operator_pubkey(&self) -> &OperatorPubKey {
        match self {
            Self::StakeChainExchange { operator_pk, .. }
            | Self::DepositSetup { operator_pk, .. }
            | Self::Musig2NoncesExchange { operator_pk, .. }
            | Self::Musig2SignaturesExchange { operator_pk, .. } => operator_pk,
        }
    }
}

/// New deposit request appeared, and operators exchanging setup data.
///
/// This is primarily used for the WOTS PKs.
#[derive(Debug, Clone)]
pub struct DepositSetup {
    /// Winternitz One Time Signature (WOTS) public keys required for processing a deposit.
    pub wots_pks: WotsPublicKeys,
}

impl DepositSetup {
    /// Tries to convert a [`ProtoDepositSetup`] into [`DepositSetup`].
    pub fn from_proto_msg(proto: &ProtoDepositSetup) -> Result<Self, DecodeError> {
        let wots_pks = WotsPublicKeys::from_flattened_bytes(&proto.wots_pks);

        Ok(Self { wots_pks })
    }
}

/// Info provided during initial startup of nodes.
///
/// This is primarily used for the Stake Chain setup.
///
/// # Implementation Details
///
/// Note that we are using little-endian encoding for the vout in [`OutPoint`]s.
#[derive(Debug, Clone)]
pub struct StakeChainExchange {
    /// [`OutPoint`] of the pre-stake transaction.
    pub pre_stake_outpoint: OutPoint,

    /// Each operator `i = 0..N` sends a message with his Schnorr verification keys `Y_{i,j}` for
    /// blocks `j = 0..M`.
    pub checkpoint_pubkeys: Vec<XOnlyPublicKey>,

    /// Stake data for a whole Stake Chain.
    pub stake_data: Vec<StakeData>,
}

impl StakeChainExchange {
    /// Tries to convert a [`ProtoStakeChainExchange`] into [`StakeChainExchange`].
    pub fn from_proto_msg(proto: &ProtoStakeChainExchange) -> Result<Self, DecodeError> {
        let mut curr = Cursor::new(&proto.pre_stake_txid);
        let txid = Decodable::consensus_decode(&mut curr)
            .map_err(|err| DecodeError::new(err.to_string()))?;

        let outpoint = OutPoint::new(txid, proto.pre_stake_vout);

        let pubkeys = proto
            .checkpoint_pubkeys
            .iter()
            .map(|bytes| XOnlyPublicKey::from_slice(bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| DecodeError::new(err.to_string()))?;

        let stake_data = proto
            .stake_data
            .iter()
            .map(|bytes| {
                let mut curr = Cursor::new(bytes);

                // Parse the Wots256PublicKey.
                // Read exactly [[u8; 20]; 68] bytes.
                const WOTS_MSG_LEN: usize = Wots256PublicKey::SIZE;
                let mut wots = [[0; WOTS_SINGLE]; WOTS_MSG_LEN]; // Wots256PublicKey
                for bytes in wots.iter_mut() {
                    curr.read_exact(bytes)
                        .map_err(|err| DecodeError::new(err.to_string()))?;
                }
                let wots = Wots256PublicKey::new(wots);

                // Parse the sha256 hash.
                let mut sha256_hash = [0; 32];
                curr.read_exact(&mut sha256_hash)
                    .map_err(|err| DecodeError::new(err.to_string()))?;
                let sha256_hash = sha256::Hash::from_bytes_ref(&sha256_hash).to_owned();

                // Parse the operator's fund Outpoint.
                let mut txid_bytes = [0; 32];
                curr.read_exact(&mut txid_bytes)
                    .map_err(|err| DecodeError::new(err.to_string()))?;
                let txid = Txid::from_slice(&txid_bytes)
                    .map_err(|err| DecodeError::new(err.to_string()))?;
                let vout = curr
                    .read_u32()
                    .map_err(|err| DecodeError::new(err.to_string()))?;
                let outpoint = OutPoint::new(txid, vout);

                Ok(StakeData {
                    withdrawal_fulfillment_pk: wots,
                    hash: sha256_hash,
                    operator_funds: outpoint,
                })
            })
            .collect::<Result<Vec<StakeData>, DecodeError>>()
            .map_err(|err| DecodeError::new(err.to_string()))?;

        Ok(Self {
            pre_stake_outpoint: outpoint,
            checkpoint_pubkeys: pubkeys,
            stake_data,
        })
    }
}

/// Unsigned messages exchanged between operators.
#[derive(Clone, Debug)]
pub enum UnsignedGossipsubMsg {
    /// Operators exchange stake chain info.
    StakeChainExchange {
        /// 32-byte hash of some unique to stake chain data.
        stake_chain_id: StakeChainId,

        /// All the necessary information to setup the stake chain.
        info: StakeChainExchange,
    },

    /// New deposit request appeared, and operators
    /// exchanging setup data.
    ///
    /// This is primarily used for the WOTS PKs.
    DepositSetup {
        /// [`Scope`] of the deposit data.
        scope: Scope,

        /// [`WotsPublicKeys`] of the deposit data.
        wots_pks: WotsPublicKeys,
    },

    /// Operators exchange (public) nonces before signing.
    Musig2NoncesExchange {
        /// [`SessionId`] of the deposit data.
        session_id: SessionId,

        /// (Public) Nonces for each transaction.
        nonces: Vec<PubNonce>,
    },

    /// Operators exchange (partial) signatures for the transaction graph.
    Musig2SignaturesExchange {
        /// [`SessionId`] of the deposit data.
        session_id: SessionId,

        /// (Partial) Signatures for each transaction.
        signatures: Vec<PartialSignature>,
    },
}

impl UnsignedGossipsubMsg {
    /// Tries to convert [`ProtoGossipsubMsgBody`] into typed [`UnsignedGossipsubMsg`]
    /// with specific types instead of raw vectors.
    pub fn from_msg_proto(proto: &ProtoGossipsubMsgBody) -> Result<Self, DecodeError> {
        let unsigned =
            match proto {
                ProtoGossipsubMsgBody::StakeChain(proto) => {
                    let bytes = proto.stake_chain_id.as_slice().try_into().map_err(|_| {
                        DecodeError::new("invalid length of bytes for stake chain id")
                    })?;
                    let stake_chain_id = StakeChainId::from_bytes(bytes);
                    Self::StakeChainExchange {
                        stake_chain_id,
                        info: StakeChainExchange::from_proto_msg(proto)?,
                    }
                }
                ProtoGossipsubMsgBody::Setup(proto) => {
                    let bytes = proto
                        .scope
                        .as_slice()
                        .try_into()
                        .map_err(|_| DecodeError::new("invalid length of bytes for scope"))?;
                    let scope = Scope::from_bytes(bytes);
                    let wots_pks = WotsPublicKeys::from_flattened_bytes(&proto.wots_pks);

                    Self::DepositSetup { scope, wots_pks }
                }
                ProtoGossipsubMsgBody::Nonce(proto) => {
                    let bytes =
                        proto.session_id.as_slice().try_into().map_err(|_| {
                            DecodeError::new("invalid length of bytes for session id")
                        })?;
                    let session_id = SessionId::from_bytes(bytes);

                    let nonces = proto
                        .pub_nonces
                        .iter()
                        .map(|bytes| PubNonce::from_bytes(bytes))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|err| DecodeError::new(err.to_string()))?;

                    Self::Musig2NoncesExchange { session_id, nonces }
                }
                ProtoGossipsubMsgBody::Sigs(proto) => {
                    let bytes =
                        proto.session_id.as_slice().try_into().map_err(|_| {
                            DecodeError::new("invalid length of bytes for session id")
                        })?;
                    let session_id = SessionId::from_bytes(bytes);

                    let partial_sigs = proto
                        .partial_sigs
                        .iter()
                        .map(|bytes| PartialSignature::from_slice(bytes))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|err| DecodeError::new(err.to_string()))?;

                    Self::Musig2SignaturesExchange {
                        session_id,
                        signatures: partial_sigs,
                    }
                }
            };

        Ok(unsigned)
    }

    /// Returns content of the message for signing.
    ///
    /// Depending on the variant, concatenates serialized data of the variant and returns it as
    /// a [`Vec`] of bytes.
    pub fn content(&self) -> Vec<u8> {
        let mut content = Vec::new();

        match &self {
            Self::StakeChainExchange {
                stake_chain_id,
                info,
            } => {
                content.extend(stake_chain_id.as_ref());
                content.extend(info.pre_stake_outpoint.vout.to_le_bytes());
                content.extend(info.pre_stake_outpoint.txid.to_byte_array());
                info.checkpoint_pubkeys.iter().for_each(|key| {
                    content.extend(key.serialize());
                });
                info.stake_data.iter().for_each(|data| {
                    content.extend(
                        data.withdrawal_fulfillment_pk
                            .iter()
                            .copied()
                            .flatten()
                            .collect::<Vec<u8>>(),
                    );
                    content.extend(data.hash.to_byte_array());
                    content.extend(data.operator_funds.txid.to_byte_array());
                    content.extend(data.operator_funds.vout.to_le_bytes());
                });
            }
            Self::DepositSetup { scope, wots_pks } => {
                content.extend(scope.as_ref());
                content.extend(wots_pks.to_flattened_bytes());
            }
            Self::Musig2NoncesExchange { session_id, nonces } => {
                content.extend(session_id.as_ref());
                for nonce in nonces {
                    content.extend(nonce.serialize());
                }
            }
            Self::Musig2SignaturesExchange {
                session_id,
                signatures,
            } => {
                content.extend(session_id.as_ref());
                for sig in signatures {
                    content.extend(sig.serialize());
                }
            }
        };

        content
    }

    /// Helper function to convert [`UnsignedGossipsubMsg`] into raw [`ProtoGossipsubMsgBody`].
    fn to_raw(&self) -> ProtoGossipsubMsgBody {
        match self {
            Self::StakeChainExchange {
                stake_chain_id,
                info,
            } => ProtoGossipsubMsgBody::StakeChain(ProtoStakeChainExchange {
                stake_chain_id: stake_chain_id.to_vec(),
                pre_stake_vout: info.pre_stake_outpoint.vout,
                pre_stake_txid: info.pre_stake_outpoint.txid.to_byte_array().to_vec(),
                checkpoint_pubkeys: info
                    .checkpoint_pubkeys
                    .iter()
                    .map(|k| k.serialize().to_vec())
                    .collect(),
                stake_data: info
                    .stake_data
                    .iter()
                    .map(|data| data.to_flattened_bytes().to_vec())
                    .collect::<Vec<Vec<u8>>>(),
            }),
            Self::DepositSetup { scope, wots_pks } => {
                ProtoGossipsubMsgBody::Setup(ProtoDepositSetup {
                    scope: scope.to_vec(),
                    wots_pks: wots_pks.to_flattened_bytes().to_vec(),
                })
            }
            Self::Musig2NoncesExchange { session_id, nonces } => {
                ProtoGossipsubMsgBody::Nonce(ProtoMusig2NoncesExchange {
                    session_id: session_id.to_vec(),
                    pub_nonces: nonces.iter().map(|n| n.serialize().to_vec()).collect(),
                })
            }
            Self::Musig2SignaturesExchange {
                session_id,
                signatures,
            } => ProtoGossipsubMsgBody::Sigs(ProtoMusig2SignaturesExchange {
                session_id: session_id.to_vec(),
                partial_sigs: signatures.iter().map(|s| s.serialize().to_vec()).collect(),
            }),
        }
    }
}

/// Gossipsub message.
#[derive(Clone, Debug)]
pub struct GossipsubMsg {
    /// Operator's signature of the message.
    pub signature: Vec<u8>,

    /// Operator's public key.
    pub key: OperatorPubKey,

    /// Unsigned payload.
    pub unsigned: UnsignedGossipsubMsg,
}

impl GossipsubMsg {
    /// Tries to decode a Gossipsub message from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let msg = ProtoGossipMsg::decode(bytes)?;
        let Some(body) = msg.body else {
            return Err(DecodeError::new("Message with empty body"));
        };

        let kind = UnsignedGossipsubMsg::from_msg_proto(&body)?;
        let key = msg.key.into();

        Ok(Self {
            signature: msg.signature,
            key,
            unsigned: kind,
        })
    }

    /// Tries to decode a Gossipsub message from a protobuf message.
    pub fn from_proto(msg: ProtoGossipMsg) -> Result<Self, DecodeError> {
        Ok(Self {
            signature: msg.signature,
            key: msg.key.into(),
            unsigned: UnsignedGossipsubMsg::from_msg_proto(&msg.body.unwrap())?,
        })
    }

    /// Converts a Gossipsub message into a protobuf message.
    pub fn into_raw(self) -> ProtoGossipMsg {
        ProtoGossipMsg {
            key: self.key.into(),
            signature: self.signature,
            body: Some(self.unsigned.to_raw()),
        }
    }

    /// Returns the content of the message as raw bytes.
    pub fn content(&self) -> Vec<u8> {
        self.unsigned.content()
    }
}
