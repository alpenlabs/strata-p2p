include!(concat!(env!("OUT_DIR"), "/strata.bitvm2.p2p.v1.rs"));

pub mod typed {
    use bitcoin::{
        consensus::Decodable,
        hashes::{sha256, Hash},
        io::Cursor,
        OutPoint, XOnlyPublicKey,
    };
    use libp2p::{identity::secp256k1::PublicKey, PeerId};
    use musig2::{PartialSignature, PubNonce};
    use prost::{DecodeError, Message};

    use super::{
        get_message_request::Body, gossipsub_msg::Body as ProtoGossipsubMsgBody,
        DepositNoncesExchange as ProtoDepositNonces, DepositRequestKey,
        DepositSetupExchange as ProtoDepositSetup, DepositSignaturesExchange as ProtoDepositSigs,
        GenesisInfo as ProtoGenesisInfo, GenesisRequestKey, GossipsubMsg as ProtoGossipMsg,
    };

    pub enum GetMessageRequestExchangeKind {
        Setup,
        Nonces,
        Signatures,
    }

    /// Typed version of "get_message_request::GetMessageRequest".
    #[allow(unused)]
    pub enum GetMessageRequest {
        Genesis {
            operator_id: PeerId,
        },
        ExchangeSession {
            scope: sha256::Hash,
            operator_id: PeerId,
            kind: GetMessageRequestExchangeKind,
        },
    }

    impl GetMessageRequest {
        pub fn from_msg(msg: super::GetMessageRequest) -> Option<GetMessageRequest> {
            let body = msg.body?;

            let (operator_id, deposit_txid, kind) = match body {
                Body::DepositSetup(DepositRequestKey { scope, operator }) => {
                    (scope, operator, GetMessageRequestExchangeKind::Setup)
                }
                Body::DepositNonce(DepositRequestKey { scope, operator }) => {
                    (scope, operator, GetMessageRequestExchangeKind::Nonces)
                }
                Body::DepositSigs(DepositRequestKey { scope, operator }) => {
                    (scope, operator, GetMessageRequestExchangeKind::Signatures)
                }
                Body::GenesisInfo(GenesisRequestKey { operator }) => {
                    return Some(Self::Genesis {
                        operator_id: PeerId::from_bytes(&operator).ok()?,
                    });
                }
            };

            let operator_id = PeerId::from_bytes(&operator_id).ok()?;
            let mut cur = Cursor::new(deposit_txid);
            let scope = Decodable::consensus_decode(&mut cur).ok()?;

            Some(Self::ExchangeSession {
                scope,
                operator_id,
                kind,
            })
        }
    }

    /// New deposit request apeared, and operators
    /// exchanging setup data.
    #[derive(Debug, Clone)]
    pub struct DepositSetup<DepositSetupPayload: Message> {
        /// Some arbitrary payload
        pub payload: DepositSetupPayload,
    }

    impl<DSP: Message + Default> DepositSetup<DSP> {
        pub fn from_proto_msg(proto: &ProtoDepositSetup) -> Result<Self, DecodeError> {
            let payload: DSP = Message::decode(proto.payload.as_ref())?;

            Ok(Self { payload })
        }
    }

    /// Operators exchange nonces before signing.
    #[derive(Debug, Clone)]
    pub struct DepositNonces {
        pub nonces: Vec<PubNonce>,
    }

    impl DepositNonces {
        pub fn from_proto_msg(proto: &ProtoDepositNonces) -> Result<Self, DecodeError> {
            let pub_nonces = proto
                .pub_nonces
                .iter()
                .map(|bytes| PubNonce::from_bytes(bytes))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| DecodeError::new(err.to_string()))?;

            Ok(Self { nonces: pub_nonces })
        }
    }

    /// Operators exchange signatures for transaction graph.
    #[derive(Debug, Clone)]
    pub struct DepositSigs {
        pub partial_sigs: Vec<PartialSignature>,
    }

    impl DepositSigs {
        pub fn from_proto_msg(proto: &ProtoDepositSigs) -> Result<Self, DecodeError> {
            let partial_sigs = proto
                .partial_sigs
                .iter()
                .map(|bytes| PartialSignature::from_slice(bytes))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| DecodeError::new(err.to_string()))?;

            Ok(Self { partial_sigs })
        }
    }

    /// Info provided during initial startup of nodes.
    #[derive(Debug, Clone)]
    pub struct GenesisInfo {
        pub pre_stake_outpoint: OutPoint,
        pub checkpoint_pubkeys: Vec<XOnlyPublicKey>,
    }

    impl GenesisInfo {
        pub fn from_proto_msg(proto: &ProtoGenesisInfo) -> Result<Self, DecodeError> {
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

            Ok(Self {
                pre_stake_outpoint: outpoint,
                checkpoint_pubkeys: pubkeys,
            })
        }
    }

    #[derive(Clone, Debug)]
    pub enum GossipsubMsgDepositKind<DepositSetupPayload: Message> {
        /// New deposit request apeared, and operators
        /// exchanging setup data.
        Setup(DepositSetup<DepositSetupPayload>),
        /// Operators exchange nonces before signing.
        Nonces(DepositNonces),
        /// Operators exchange signatures for transaction graph.
        Sigs(DepositSigs),
    }

    impl<DepositSetupPayload: Message> From<DepositSigs>
        for GossipsubMsgDepositKind<DepositSetupPayload>
    {
        fn from(v: DepositSigs) -> Self {
            Self::Sigs(v)
        }
    }

    impl<DepositSetupPayload: Message> From<DepositNonces>
        for GossipsubMsgDepositKind<DepositSetupPayload>
    {
        fn from(v: DepositNonces) -> Self {
            Self::Nonces(v)
        }
    }

    impl<DepositSetupPayload: Message> From<DepositSetup<DepositSetupPayload>>
        for GossipsubMsgDepositKind<DepositSetupPayload>
    {
        fn from(v: DepositSetup<DepositSetupPayload>) -> Self {
            Self::Setup(v)
        }
    }

    #[derive(Clone, Debug)]
    pub enum GossipsubMsgKind<DepositSetupPayload: Message> {
        /// Operators exchange
        GenesisInfo(GenesisInfo),
        Deposit {
            scope: sha256::Hash,
            kind: GossipsubMsgDepositKind<DepositSetupPayload>,
        },
    }

    impl<DSP: Message + Default> GossipsubMsgKind<DSP> {
        pub fn from_msg_proto(proto: &ProtoGossipsubMsgBody) -> Result<Self, DecodeError> {
            let (scope, kind) = match proto {
                ProtoGossipsubMsgBody::GenesisInfo(proto) => {
                    return Ok(Self::GenesisInfo(GenesisInfo::from_proto_msg(proto)?));
                }
                ProtoGossipsubMsgBody::Setup(proto) => {
                    (&proto.scope, DepositSetup::from_proto_msg(proto)?.into())
                }
                ProtoGossipsubMsgBody::Nonce(proto) => {
                    (&proto.scope, DepositNonces::from_proto_msg(proto)?.into())
                }
                ProtoGossipsubMsgBody::Sigs(proto) => {
                    (&proto.scope, DepositSigs::from_proto_msg(proto)?.into())
                }
            };

            let mut curr = Cursor::new(scope);
            let scope = Decodable::consensus_decode(&mut curr)
                .map_err(|err| DecodeError::new(err.to_string()))?;

            Ok(Self::Deposit { scope, kind })
        }

        /// Return content of the message for signing.
        ///
        /// Depending on the variant, concatenates serialized data of the variant and returns it as
        /// a vec of bytes.
        pub fn content(&self) -> Vec<u8> {
            let mut content = Vec::new();

            match &self {
                GossipsubMsgKind::GenesisInfo(GenesisInfo {
                    pre_stake_outpoint,
                    checkpoint_pubkeys,
                }) => {
                    content.extend(pre_stake_outpoint.vout.to_le_bytes());
                    content.extend(pre_stake_outpoint.txid.to_byte_array());
                    checkpoint_pubkeys.iter().for_each(|key| {
                        content.extend(key.serialize());
                    });
                }
                GossipsubMsgKind::Deposit { scope, kind } => {
                    content.extend(scope.to_byte_array());

                    match kind {
                        GossipsubMsgDepositKind::Setup(DepositSetup { payload }) => {
                            content.extend(payload.encode_to_vec());
                        }
                        GossipsubMsgDepositKind::Nonces(DepositNonces { nonces }) => {
                            for nonce in nonces {
                                content.extend(nonce.serialize());
                            }
                        }
                        GossipsubMsgDepositKind::Sigs(DepositSigs { partial_sigs }) => {
                            for sig in partial_sigs {
                                content.extend(sig.serialize());
                            }
                        }
                    };
                }
            };

            content
        }

        fn to_raw(&self) -> ProtoGossipsubMsgBody {
            match self {
                GossipsubMsgKind::GenesisInfo(info) => {
                    ProtoGossipsubMsgBody::GenesisInfo(ProtoGenesisInfo {
                        pre_stake_vout: info.pre_stake_outpoint.vout,
                        pre_stake_txid: info.pre_stake_outpoint.txid.to_byte_array().to_vec(),
                        checkpoint_pubkeys: info
                            .checkpoint_pubkeys
                            .iter()
                            .map(|k| k.serialize().to_vec())
                            .collect(),
                    })
                }
                GossipsubMsgKind::Deposit { scope, kind } => {
                    let scope = scope.to_byte_array().to_vec();
                    match kind {
                        GossipsubMsgDepositKind::Setup(setup) => {
                            ProtoGossipsubMsgBody::Setup(ProtoDepositSetup {
                                scope,
                                payload: setup.payload.encode_to_vec(),
                            })
                        }
                        GossipsubMsgDepositKind::Nonces(dep) => {
                            ProtoGossipsubMsgBody::Nonce(ProtoDepositNonces {
                                scope,
                                pub_nonces: dep
                                    .nonces
                                    .iter()
                                    .map(|n| n.serialize().to_vec())
                                    .collect(),
                            })
                        }
                        GossipsubMsgDepositKind::Sigs(dep) => {
                            ProtoGossipsubMsgBody::Sigs(ProtoDepositSigs {
                                scope,
                                partial_sigs: dep
                                    .partial_sigs
                                    .iter()
                                    .map(|s| s.serialize().to_vec())
                                    .collect(),
                            })
                        }
                    }
                }
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct GossipsubMsg<DepositSetupPayload: Message + Clone> {
        pub signature: Vec<u8>,
        pub key: PublicKey,
        pub kind: GossipsubMsgKind<DepositSetupPayload>,
    }

    impl<DepositSetupPayload> GossipsubMsg<DepositSetupPayload>
    where
        DepositSetupPayload: Message + Default + Clone,
    {
        pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
            let msg = super::GossipsubMsg::decode(bytes)?;
            let Some(body) = msg.body else {
                return Err(DecodeError::new("Message with empty body"));
            };

            let kind = GossipsubMsgKind::<DepositSetupPayload>::from_msg_proto(&body)?;
            let key = PublicKey::try_from_bytes(&msg.key)
                .map_err(|err| DecodeError::new(err.to_string()))?;

            Ok(Self {
                signature: msg.signature,
                key,
                kind,
            })
        }

        pub fn into_raw(self) -> ProtoGossipMsg {
            ProtoGossipMsg {
                key: self.key.to_bytes().to_vec(),
                signature: self.signature,
                body: Some(self.kind.to_raw()),
            }
        }

        pub fn content(&self) -> Vec<u8> {
            self.kind.content()
        }
    }
}