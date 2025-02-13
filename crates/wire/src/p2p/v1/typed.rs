//! Types derived from the `.proto` files.

use bitcoin::{
    consensus::Decodable,
    hashes::{sha256, Hash},
    io::{Cursor, Read},
    OutPoint, XOnlyPublicKey,
};
use musig2::{PartialSignature, PubNonce};
use prost::{DecodeError, Message};
use strata_p2p_types::{OperatorPubKey, Scope, SessionId, Wots256PublicKey};

use super::proto::{
    get_message_request::Body as ProtoGetMessageRequestBody,
    gossipsub_msg::Body as ProtoGossipsubMsgBody, DepositRequestKey,
    DepositSetupExchange as ProtoDepositSetup, GetMessageRequest as ProtoGetMessageRequest,
    GossipsubMsg as ProtoGossipMsg, Musig2ExchangeRequestKey,
    Musig2NoncesExchange as ProtoMusig2NoncesExchange,
    Musig2SignaturesExchange as ProtoMusig2SignaturesExchange,
    StakeChainExchange as ProtoStakeChainExchange, StakeChainRequestKey,
};

/// Typed version of "get_message_request::GetMessageRequest".
#[derive(Clone, Debug)]
pub enum GetMessageRequest {
    /// Request Stake Chain info for this operator.
    StakeChainExchange { operator_pk: OperatorPubKey },

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
            ProtoGetMessageRequestBody::Nonces(Musig2ExchangeRequestKey {
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
            ProtoGetMessageRequestBody::Sigs(Musig2ExchangeRequestKey {
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
            ProtoGetMessageRequestBody::StakeChain(StakeChainRequestKey { operator }) => {
                Self::StakeChainExchange {
                    operator_pk: operator.into(),
                }
            }
        };

        Ok(request)
    }

    /// Converts [`GetMessageRequest`] into raw [`ProtoGetMessageRequest`].
    pub fn into_msg(self) -> ProtoGetMessageRequest {
        let body = match self {
            Self::StakeChainExchange { operator_pk } => {
                ProtoGetMessageRequestBody::StakeChain(StakeChainRequestKey {
                    operator: operator_pk.into(),
                })
            }
            Self::DepositSetup { scope, operator_pk } => {
                ProtoGetMessageRequestBody::DepositSetup(DepositRequestKey {
                    scope: scope.to_vec(),
                    operator: operator_pk.into(),
                })
            }
            Self::Musig2SignaturesExchange {
                session_id,
                operator_pk,
            } => ProtoGetMessageRequestBody::Nonces(Musig2ExchangeRequestKey {
                session_id: session_id.to_vec(),
                operator: operator_pk.into(),
            }),
            Self::Musig2NoncesExchange {
                session_id,
                operator_pk,
            } => ProtoGetMessageRequestBody::Sigs(Musig2ExchangeRequestKey {
                session_id: session_id.to_vec(),
                operator: operator_pk.into(),
            }),
        };

        ProtoGetMessageRequest { body: Some(body) }
    }

    /// Returns the [`OperatorPubKey`] with respect to this [`GetMessageRequest`].
    pub fn operator_pubkey(&self) -> &OperatorPubKey {
        match self {
            Self::StakeChainExchange { operator_pk }
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
pub struct DepositSetup<DepositSetupPayload: Message> {
    /// Some arbitrary payload.
    ///
    /// Primarily used for the WOTS PKs.
    pub payload: DepositSetupPayload,
}

impl<DSP: Message + Default> DepositSetup<DSP> {
    /// Tries to convert a [`ProtoDepositSetup`] into [`DepositSetup`].
    pub fn from_proto_msg(proto: &ProtoDepositSetup) -> Result<Self, DecodeError> {
        let payload: DSP = Message::decode(proto.payload.as_ref())?;

        Ok(Self { payload })
    }
}

/// Info provided during initial startup of nodes.
///
/// This is primarily used for the Stake Chain setup.
#[derive(Debug, Clone)]
pub struct StakeChainExchange {
    /// [`OutPoint`] of the pre-stake transaction.
    pub pre_stake_outpoint: OutPoint,

    /// Each operator `i = 0..N` sends a message with his Schnorr verification keys `Y_{i,j}` for
    /// blocks `j = 0..M`.
    pub checkpoint_pubkeys: Vec<XOnlyPublicKey>,

    /// WOTS public keys for each stake transaction.
    pub stake_wots: Vec<Wots256PublicKey>,

    /// Hashes for each stake transaction.
    pub stake_hashes: Vec<sha256::Hash>,

    /// Operator's funds prevouts for each stake transaction.
    ///
    /// There are to cover the dust outputs in each withdrawal request.
    /// Composed of `txid:vout` ([`OutPoint`]) as a flattened byte array.
    pub operator_funds: Vec<OutPoint>,
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

        let stake_wots = proto
            .stake_wots
            .iter()
            .map(|bytes| {
                let mut curr = Cursor::new(bytes);
                // Read exactly [[u8; 20]; 256] bytes.
                let mut wots = [[0; 20]; 256]; // Wots256PublicKey
                for bytes in wots.iter_mut() {
                    curr.read_exact(bytes)
                        .map_err(|err| DecodeError::new(err.to_string()))?;
                }
                Ok(Wots256PublicKey::new(wots))
            })
            .collect::<Result<Vec<Wots256PublicKey>, DecodeError>>()
            .map_err(|err| DecodeError::new(err.to_string()))?;

        let stake_hashes = proto
            .stake_hashes
            .iter()
            .map(|bytes| sha256::Hash::from_slice(bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| DecodeError::new(err.to_string()))?;

        let operator_funds = proto
            .operator_funds
            .iter()
            .map(|bytes| {
                let mut curr = Cursor::new(bytes);
                let outpoint = Decodable::consensus_decode(&mut curr)
                    .map_err(|err| DecodeError::new(err.to_string()))?;
                Ok(outpoint)
            })
            .collect::<Result<Vec<OutPoint>, DecodeError>>()
            .map_err(|err| DecodeError::new(err.to_string()))?;

        Ok(Self {
            pre_stake_outpoint: outpoint,
            checkpoint_pubkeys: pubkeys,
            stake_wots,
            stake_hashes,
            operator_funds,
        })
    }
}

/// Unsigned messages exchanged between operators.
#[derive(Clone, Debug)]
pub enum UnsignedGossipsubMsg<DepositSetupPayload: Message> {
    /// Operators exchange stake chain info.
    StakeChainExchange(StakeChainExchange),

    /// New deposit request appeared, and operators
    /// exchanging setup data.
    ///
    /// This is primarily used for the WOTS PKs.
    DepositSetup {
        /// [`Scope`] of the deposit data.
        scope: Scope,

        /// Payload of the deposit setup.
        setup: DepositSetup<DepositSetupPayload>,
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

impl<DSP: Message + Default> UnsignedGossipsubMsg<DSP> {
    /// Tries to convert [`ProtoGossipsubMsgBody`] into typed [`UnsignedGossipsubMsg`]
    /// with specific types instead of raw vectors.
    pub fn from_msg_proto(proto: &ProtoGossipsubMsgBody) -> Result<Self, DecodeError> {
        let unsigned =
            match proto {
                ProtoGossipsubMsgBody::StakeChain(proto) => {
                    Self::StakeChainExchange(StakeChainExchange::from_proto_msg(proto)?)
                }
                ProtoGossipsubMsgBody::Setup(proto) => {
                    let bytes = proto
                        .scope
                        .as_slice()
                        .try_into()
                        .map_err(|_| DecodeError::new("invalid length of bytes for scope"))?;
                    let scope = Scope::from_bytes(bytes);

                    Self::DepositSetup {
                        scope,
                        setup: DepositSetup::from_proto_msg(proto)?,
                    }
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
            Self::StakeChainExchange(StakeChainExchange {
                pre_stake_outpoint,
                checkpoint_pubkeys,
                stake_wots,
                stake_hashes,
                operator_funds,
            }) => {
                content.extend(pre_stake_outpoint.vout.to_le_bytes());
                content.extend(pre_stake_outpoint.txid.to_byte_array());
                checkpoint_pubkeys.iter().for_each(|key| {
                    content.extend(key.serialize());
                });
                stake_wots.iter().for_each(|wots| {
                    content.extend(wots.iter().copied().flatten().collect::<Vec<u8>>());
                });
                stake_hashes.iter().for_each(|hash| {
                    content.extend(hash.to_byte_array());
                });
                operator_funds.iter().for_each(|outpoint| {
                    content.extend(outpoint.txid.to_byte_array());
                    content.extend(outpoint.vout.to_le_bytes());
                });
            }
            Self::DepositSetup { scope, setup } => {
                content.extend(scope.as_ref());
                content.extend(setup.payload.encode_to_vec());
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
            Self::StakeChainExchange(info) => {
                ProtoGossipsubMsgBody::StakeChain(ProtoStakeChainExchange {
                    pre_stake_vout: info.pre_stake_outpoint.vout,
                    pre_stake_txid: info.pre_stake_outpoint.txid.to_byte_array().to_vec(),
                    checkpoint_pubkeys: info
                        .checkpoint_pubkeys
                        .iter()
                        .map(|k| k.serialize().to_vec())
                        .collect(),
                    stake_wots: info
                        .stake_wots
                        .iter()
                        .map(|w| w.iter().copied().flatten().collect::<Vec<u8>>())
                        .collect(),
                    stake_hashes: info
                        .stake_hashes
                        .iter()
                        .map(|h| h.to_byte_array().to_vec())
                        .collect(),
                    operator_funds: info
                        .operator_funds
                        .iter()
                        .map(|op| op.to_string().as_bytes().to_vec())
                        .collect(),
                })
            }
            Self::DepositSetup { scope, setup } => {
                ProtoGossipsubMsgBody::Setup(ProtoDepositSetup {
                    scope: scope.to_vec(),
                    payload: setup.payload.encode_to_vec(),
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
pub struct GossipsubMsg<DepositSetupPayload: Message + Clone> {
    /// Operator's signature of the message.
    pub signature: Vec<u8>,

    /// Operator's public key.
    pub key: OperatorPubKey,

    /// Unsigned payload.
    pub unsigned: UnsignedGossipsubMsg<DepositSetupPayload>,
}

impl<DepositSetupPayload> GossipsubMsg<DepositSetupPayload>
where
    DepositSetupPayload: Message + Default + Clone,
{
    /// Tries to decode a Gossipsub message from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let msg = ProtoGossipMsg::decode(bytes)?;
        let Some(body) = msg.body else {
            return Err(DecodeError::new("Message with empty body"));
        };

        let kind = UnsignedGossipsubMsg::<DepositSetupPayload>::from_msg_proto(&body)?;
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
