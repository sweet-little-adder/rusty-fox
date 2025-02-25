use kaspa_consensus_core::tx::TransactionId;
use std::num::TryFromIntError;
use thiserror::Error;

use crate::{RpcHash, RpcTransactionId};

#[derive(Clone, Debug, Error)]
pub enum RpcError {
    #[error("Not implemented")]
    NotImplemented,

    #[error("Integer downsize conversion error {0}")]
    IntConversionError(#[from] TryFromIntError),

    #[error("Hex parsing error: {0}")]
    HexParsingError(#[from] faster_hex::Error),

    #[error("Blue work parsing error {0}")]
    RpcBlueWorkTypeParseError(std::num::ParseIntError),

    #[error("Integer parsing error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("Invalid script class: {0}")]
    InvalidRpcScriptClass(String),

    #[error("Missing required field {0}.{1}")]
    MissingRpcFieldError(String, String),

    #[error("Feature not supported")]
    UnsupportedFeature,

    #[error("Primitive to enum conversion error")]
    PrimitiveToEnumConversionError,

    #[error("Coinbase payload is above max length ({0}). Try to shorten the extra data.")]
    CoinbasePayloadLengthAboveMax(usize),

    #[error("Rejected transaction {0}: {1}")]
    RejectedTransaction(RpcTransactionId, String),

    #[error("Block {0} is invalid. No verbose data can be built.")]
    InvalidBlock(RpcHash),

    #[error("If includeTransactions is set, then includeBlockVerboseData must be set as well.")]
    InvalidGetBlocksRequest,

    #[error("Transaction {0} not found")]
    TransactionNotFound(TransactionId),

    #[error("Method unavailable. Run the node with the --utxoindex argument.")]
    NoUtxoIndex,

    #[error(transparent)]
    AddressError(#[from] kaspa_addresses::AddressError),

    #[error(transparent)]
    NetworkTypeError(#[from] kaspa_consensus_core::networktype::NetworkTypeError),

    #[error(transparent)]
    NotificationError(#[from] kaspa_notify::error::Error),

    #[error(transparent)]
    MiningManagerError(#[from] kaspa_mining::errors::MiningManagerError),

    #[error(transparent)]
    ConsensusError(#[from] kaspa_consensus_core::errors::consensus::ConsensusError),

    #[error(transparent)]
    ScriptClassError(#[from] kaspa_txscript::script_class::Error),

    #[error("{0}")]
    General(String),
}

impl From<String> for RpcError {
    fn from(value: String) -> Self {
        RpcError::General(value)
    }
}

impl From<&str> for RpcError {
    fn from(value: &str) -> Self {
        RpcError::General(value.to_string())
    }
}

pub type RpcResult<T> = std::result::Result<T, crate::RpcError>;
