use pallas::ledger::traverse::MultiEraOutput;
use pallas::ledger::traverse::{MultiEraBlock, MultiEraTx, OutputRef};
use serde::Deserialize;

use crate::{crosscut, model, prelude::*};

#[derive(Deserialize)]
pub struct Config {
    pub key_prefix: Option<String>,
    pub filter: Option<Vec<String>>,
}

pub struct Reducer {
    config: Config,
    policy: crosscut::policies::RuntimePolicy,
}

impl Reducer {
    fn process_inbound_txo(
        &mut self,
        ctx: &model::BlockContext,
        input: &OutputRef,
        output: &mut super::OutputPort,
    ) -> Result<(), gasket::error::Error> {
        let utxo = ctx.find_utxo(input).apply_policy(&self.policy).or_panic()?;

        let utxo = match utxo {
            Some(x) => x,
            None => return Ok(()),
        };

        let address = utxo.address().map(|x| x.to_string()).or_panic()?;

        if let Some(addresses) = &self.config.filter {
            if let Err(_) = addresses.binary_search(&address) {
                return Ok(());
            }
        }

        let crdt = model::CRDTCommand::set_remove(
            self.config.key_prefix.as_deref(),
            &address,
            input.to_string(),
        );

        output.send(crdt.into())
    }

    fn process_outbound_txo(
        &mut self,
        tx: &MultiEraTx,
        tx_output: &MultiEraOutput,
        output_idx: usize,
        output: &mut super::OutputPort,
    ) -> Result<(), gasket::error::Error> {
        let tx_hash = tx.hash();
        let address = tx_output.address().map(|x| x.to_string()).or_panic()?;

        if let Some(addresses) = &self.config.filter {
            if let Err(_) = addresses.binary_search(&address) {
                return Ok(());
            }
        }

        let crdt = model::CRDTCommand::set_add(
            self.config.key_prefix.as_deref(),
            &address,
            format!("{}#{}", tx_hash, output_idx),
        );

        output.send(crdt.into())
    }

    pub fn reduce_valid_tx(
        &mut self,
        tx: &MultiEraTx,
        ctx: &model::BlockContext,
        output: &mut super::OutputPort,
    ) -> Result<(), gasket::error::Error> {
        for input in tx.inputs().iter().map(|i| i.output_ref()) {
            self.process_inbound_txo(&ctx, &input, output)?;
        }

        for (idx, tx_output) in tx.outputs().iter().enumerate() {
            self.process_outbound_txo(tx, tx_output, idx, output)?;
        }

        Ok(())
    }

    pub fn reduce_invalid_tx<'b>(
        &mut self,
        tx: &MultiEraTx,
        ctx: &model::BlockContext,
        output: &mut super::OutputPort,
    ) -> Result<(), gasket::error::Error> {
        for input in tx.collateral().iter().map(|i| i.output_ref()) {
            self.process_inbound_txo(&ctx, &input, output)?;
        }
        
        if let Some(coll_ret) = tx.collateral_return() {
            let idx = tx.outputs().len();
            self.process_outbound_txo(tx, &coll_ret, idx, output)?;
        }

        Ok(())
    }

    pub fn reduce_block<'b>(
        &mut self,
        block: &'b MultiEraBlock<'b>,
        ctx: &model::BlockContext,
        output: &mut super::OutputPort,
    ) -> Result<(), gasket::error::Error> {
        for tx in block.txs().into_iter() {
            match tx.is_valid() {
                true => self.reduce_valid_tx(&tx, ctx, output)?,
                false => self.reduce_invalid_tx(&tx, ctx, output)?,
            };
        }

        Ok(())
    }
}

impl Config {
    pub fn plugin(self, policy: &crosscut::policies::RuntimePolicy) -> super::Reducer {
        let reducer = Reducer {
            config: self,
            policy: policy.clone(),
        };

        super::Reducer::UtxoByAddress(reducer)
    }
}
