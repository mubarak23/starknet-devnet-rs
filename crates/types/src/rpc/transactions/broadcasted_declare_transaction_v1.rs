use cairo_felt::Felt252;
use serde::{Deserialize, Serialize};
use starknet_api::transaction::Fee;
use starknet_in_rust::core::transaction_hash::{
    calculate_transaction_hash_common, TransactionHashPrefix as SirTransactionHashPrefix,
};
use starknet_in_rust::definitions::constants::VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;
use starknet_in_rust::transaction::{verify_version, Declare as SirDeclare};

use crate::contract_address::ContractAddress;
use crate::contract_class::Cairo0ContractClass;
use crate::error::DevnetResult;
use crate::felt::{
    ClassHash, Felt, Nonce, TransactionHash, TransactionSignature, TransactionVersion,
};
use crate::rpc::transactions::declare_transaction_v0v1::DeclareTransactionV0V1;
use crate::rpc::transactions::BroadcastedTransactionCommon;
use crate::traits::HashProducer;

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct BroadcastedDeclareTransactionV1 {
    #[serde(flatten)]
    pub common: BroadcastedTransactionCommon,
    pub contract_class: Cairo0ContractClass,
    pub sender_address: ContractAddress,
}

impl BroadcastedDeclareTransactionV1 {
    pub fn new(
        sender_address: ContractAddress,
        max_fee: Fee,
        signature: &TransactionSignature,
        nonce: Nonce,
        contract_class: &Cairo0ContractClass,
        version: TransactionVersion,
    ) -> Self {
        Self {
            sender_address,
            contract_class: contract_class.clone(),
            common: BroadcastedTransactionCommon {
                max_fee,
                nonce,
                version,
                signature: signature.clone(),
            },
        }
    }

    pub fn create_sir_declare(
        &self,
        class_hash: ClassHash,
        transaction_hash: TransactionHash,
    ) -> DevnetResult<SirDeclare> {
        let declare = SirDeclare {
            class_hash: class_hash.into(),
            sender_address: self.sender_address.into(),
            validate_entry_point_selector: VALIDATE_DECLARE_ENTRY_POINT_SELECTOR.clone(),
            version: self.common.version.into(),
            max_fee: self.common.max_fee.0,
            signature: self.common.signature.iter().map(|felt| felt.into()).collect(),
            nonce: self.common.nonce.into(),
            hash_value: transaction_hash.into(),
            contract_class: self.contract_class.clone().try_into()?, /* ? Not present in
                                                                      * DeclareTransactionV0V1 */
            skip_execute: false,
            skip_fee_transfer: false,
            skip_validate: false,
        };

        verify_version(&declare.version, declare.max_fee, &declare.nonce, &declare.signature)?;

        Ok(declare)
    }

    pub fn create_declare(
        &self,
        class_hash: ClassHash,
        transaction_hash: TransactionHash,
    ) -> DeclareTransactionV0V1 {
        DeclareTransactionV0V1 {
            class_hash,
            sender_address: self.sender_address,
            nonce: self.common.nonce,
            max_fee: self.common.max_fee,
            version: self.common.version,
            transaction_hash,
            signature: self.common.signature.clone(),
        }
    }

    pub fn generate_class_hash(&self) -> DevnetResult<Felt> {
        self.contract_class.generate_hash()
    }

    pub fn calculate_transaction_hash(
        &self,
        chain_id: &Felt,
        class_hash: &ClassHash,
    ) -> DevnetResult<ClassHash> {
        let additional_data: Vec<Felt252> = vec![self.common.nonce.into()];
        let calldata = vec![class_hash.into()];
        // TODO: Remove when SirDeclare::new will give same hash
        Ok(calculate_transaction_hash_common(
            SirTransactionHashPrefix::Declare,
            self.common.version.into(),
            &self.sender_address.into(),
            Felt252::from(0),
            &calldata,
            self.common.max_fee.0,
            chain_id.into(),
            &additional_data,
        )?
        .into())
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use starknet_api::transaction::Fee;
    use starknet_in_rust::definitions::block_context::StarknetChainId;

    use crate::contract_address::ContractAddress;
    use crate::contract_class::Cairo0Json;
    use crate::felt::Felt;
    use crate::rpc::transactions::broadcasted_declare_transaction_v1::BroadcastedDeclareTransactionV1;
    use crate::traits::{HashProducer, ToHexString};

    #[derive(Deserialize)]
    struct FeederGatewayDeclareTransactionV1 {
        transaction_hash: Felt,
        max_fee: Felt,
        nonce: Felt,
        class_hash: Felt,
        sender_address: Felt,
        version: Felt,
    }

    #[test]
    /// test_artifact is taken from starknet-rs. https://github.com/xJonathanLEI/starknet-rs/blob/starknet-core/v0.5.1/starknet-core/test-data/contracts/cairo0/artifacts/event_example.txt
    fn correct_transaction_hash_computation_compared_to_a_transaction_from_feeder_gateway() {
        let json_str = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/events_cairo0.casm"
        ))
        .unwrap();
        let cairo0 = Cairo0Json::raw_json_from_json_str(&json_str).unwrap();

        // this is declare v1 transaction send with starknet-rs
        let json_obj: serde_json::Value = serde_json::from_reader(std::fs::File::open(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/sequencer_response/declare_v1_testnet_0x04f3480733852ec616431fd89a5e3127b49cef0ac7a71440ebdec40b1322ca9d.json"
        )).unwrap()).unwrap();

        let feeder_gateway_transaction: FeederGatewayDeclareTransactionV1 =
            serde_json::from_value(json_obj.get("transaction").unwrap().clone()).unwrap();

        assert_eq!(feeder_gateway_transaction.class_hash, cairo0.generate_hash().unwrap());

        let broadcasted_tx = BroadcastedDeclareTransactionV1::new(
            ContractAddress::new(feeder_gateway_transaction.sender_address).unwrap(),
            Fee(u128::from_str_radix(
                &feeder_gateway_transaction.max_fee.to_nonprefixed_hex_str(),
                16,
            )
            .unwrap()),
            &vec![],
            feeder_gateway_transaction.nonce,
            &cairo0.into(),
            feeder_gateway_transaction.version,
        );

        let class_hash = broadcasted_tx.generate_class_hash().unwrap();
        let transaction_hash = broadcasted_tx
            .calculate_transaction_hash(&StarknetChainId::TestNet.to_felt().into(), &class_hash)
            .unwrap();

        let sir_declare_transaction =
            broadcasted_tx.create_sir_declare(class_hash, transaction_hash).unwrap();

        assert_eq!(
            feeder_gateway_transaction.transaction_hash,
            sir_declare_transaction.hash_value.into()
        );
    }
}