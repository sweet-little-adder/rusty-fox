use kaspa_consensus_core::{
    tx::{TransactionIndexType, TransactionOutpoint, UtxoEntry},
    utxo::{
        utxo_diff::{ImmutableUtxoDiff, UtxoDiff},
        utxo_view::UtxoView,
    },
};
use kaspa_database::prelude::StoreError;
use kaspa_database::prelude::StoreResultExtensions;
use kaspa_database::prelude::DB;
use kaspa_database::prelude::{BatchDbWriter, CachedDbAccess, DirectDbWriter};
use kaspa_hashes::Hash;
use rocksdb::WriteBatch;
use std::{error::Error, fmt::Display, sync::Arc};

type UtxoCollectionIterator<'a> = Box<dyn Iterator<Item = Result<(TransactionOutpoint, UtxoEntry), Box<dyn Error>>> + 'a>;

pub trait UtxoSetStoreReader {
    fn get(&self, outpoint: &TransactionOutpoint) -> Result<Arc<UtxoEntry>, StoreError>;

    fn seek_iterator(&self, from_outpoint: Option<TransactionOutpoint>, limit: usize, skip_first: bool) -> UtxoCollectionIterator;
}

pub trait UtxoSetStore: UtxoSetStoreReader {
    /// Updates the store according to the UTXO diff -- adding and deleting entries correspondingly.
    /// Note we define `self` as `mut` in order to require write access even though the compiler does not require it.
    /// This is because concurrent readers can interfere with cache consistency.  
    fn write_diff(&mut self, utxo_diff: &UtxoDiff) -> Result<(), StoreError>;
    fn write_many(&mut self, utxos: &[(TransactionOutpoint, UtxoEntry)]) -> Result<(), StoreError>;
}

pub const UTXO_KEY_SIZE: usize = kaspa_hashes::HASH_SIZE + std::mem::size_of::<TransactionIndexType>();

#[derive(Eq, Hash, PartialEq, Debug, Copy, Clone)]
struct UtxoKey([u8; UTXO_KEY_SIZE]);

impl AsRef<[u8]> for UtxoKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for UtxoKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outpoint: TransactionOutpoint = (*self).into();
        outpoint.fmt(f)
    }
}

impl From<TransactionOutpoint> for UtxoKey {
    fn from(outpoint: TransactionOutpoint) -> Self {
        let mut bytes = [0; UTXO_KEY_SIZE];
        bytes[..kaspa_hashes::HASH_SIZE].copy_from_slice(&outpoint.transaction_id.as_bytes());
        bytes[kaspa_hashes::HASH_SIZE..].copy_from_slice(&outpoint.index.to_le_bytes());
        Self(bytes)
    }
}

impl From<UtxoKey> for TransactionOutpoint {
    fn from(k: UtxoKey) -> Self {
        let transaction_id = Hash::from_slice(&k.0[..kaspa_hashes::HASH_SIZE]);
        let index = TransactionIndexType::from_le_bytes(
            <[u8; std::mem::size_of::<TransactionIndexType>()]>::try_from(&k.0[kaspa_hashes::HASH_SIZE..])
                .expect("expecting index size"),
        );
        Self::new(transaction_id, index)
    }
}

#[derive(Clone)]
pub struct DbUtxoSetStore {
    db: Arc<DB>,
    prefix: &'static [u8],
    access: CachedDbAccess<UtxoKey, Arc<UtxoEntry>>,
}

impl DbUtxoSetStore {
    pub fn new(db: Arc<DB>, cache_size: u64, prefix: &'static [u8]) -> Self {
        Self { db: Arc::clone(&db), access: CachedDbAccess::new(Arc::clone(&db), cache_size, prefix.to_vec()), prefix }
    }

    pub fn clone_with_new_cache(&self, cache_size: u64) -> Self {
        Self::new(Arc::clone(&self.db), cache_size, self.prefix)
    }

    /// See comment at [`UtxoSetStore::write_diff`]
    pub fn write_diff_batch(&mut self, batch: &mut WriteBatch, utxo_diff: &impl ImmutableUtxoDiff) -> Result<(), StoreError> {
        let mut writer = BatchDbWriter::new(batch);
        self.access.delete_many(&mut writer, &mut utxo_diff.removed().keys().map(|o| (*o).into()))?;
        self.access.write_many(&mut writer, &mut utxo_diff.added().iter().map(|(o, e)| ((*o).into(), Arc::new(e.clone()))))?;
        Ok(())
    }

    pub fn iterator(&self) -> impl Iterator<Item = Result<(TransactionOutpoint, Arc<UtxoEntry>), Box<dyn Error>>> + '_ {
        self.access.iterator().map(|iter_result| match iter_result {
            Ok((key_bytes, utxo_entry)) => match <[u8; UTXO_KEY_SIZE]>::try_from(&key_bytes[..]) {
                Ok(utxo_key_slice) => {
                    let utxo_key = UtxoKey(utxo_key_slice);
                    let outpoint: TransactionOutpoint = utxo_key.into();
                    Ok((outpoint, utxo_entry))
                }
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e),
        })
    }

    /// Clear the store completely in DB and cache
    pub fn clear(&mut self) -> Result<(), StoreError> {
        let writer = DirectDbWriter::new(&self.db);
        self.access.delete_all(writer)
    }

    /// Write directly from an iterator and do not cache any data. NOTE: this action also clears the cache
    pub fn write_from_iterator_without_cache(
        &mut self,
        utxos: impl IntoIterator<Item = (TransactionOutpoint, Arc<UtxoEntry>)>,
    ) -> Result<(), StoreError> {
        let mut writer = DirectDbWriter::new(&self.db);
        self.access.write_many_without_cache(&mut writer, &mut utxos.into_iter().map(|(o, e)| (o.into(), e)))?;
        Ok(())
    }
}

impl UtxoView for DbUtxoSetStore {
    fn get(&self, outpoint: &TransactionOutpoint) -> Option<UtxoEntry> {
        UtxoSetStoreReader::get(self, outpoint).map(|v| v.as_ref().clone()).unwrap_option()
    }
}

impl UtxoSetStoreReader for DbUtxoSetStore {
    fn get(&self, outpoint: &TransactionOutpoint) -> Result<Arc<UtxoEntry>, StoreError> {
        self.access.read((*outpoint).into())
    }

    fn seek_iterator(&self, from_outpoint: Option<TransactionOutpoint>, limit: usize, skip_first: bool) -> UtxoCollectionIterator {
        let seek_key = from_outpoint.map(UtxoKey::from);
        Box::new(self.access.seek_iterator(None, seek_key, limit, skip_first).map(|res| {
            let (key, entry) = res?;
            let outpoint: TransactionOutpoint = UtxoKey(<[u8; UTXO_KEY_SIZE]>::try_from(&key[..]).unwrap()).into();
            Ok((outpoint, UtxoEntry::clone(&entry)))
        }))
    }
}

impl UtxoSetStore for DbUtxoSetStore {
    fn write_diff(&mut self, utxo_diff: &UtxoDiff) -> Result<(), StoreError> {
        let mut writer = DirectDbWriter::new(&self.db);
        self.access.delete_many(&mut writer, &mut utxo_diff.removed().keys().map(|o| (*o).into()))?;
        self.access.write_many(&mut writer, &mut utxo_diff.added().iter().map(|(o, e)| ((*o).into(), Arc::new(e.clone()))))?;
        Ok(())
    }

    fn write_many(&mut self, utxos: &[(TransactionOutpoint, UtxoEntry)]) -> Result<(), StoreError> {
        let mut writer = DirectDbWriter::new(&self.db);
        self.access.write_many(&mut writer, &mut utxos.iter().map(|(o, e)| ((*o).into(), Arc::new(e.clone()))))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;

    #[test]
    fn test_utxo_key_conversion() {
        let id = 2345.into();
        let outpoint = TransactionOutpoint::new(id, 300);
        let key: UtxoKey = outpoint.into();
        assert_eq!(outpoint, key.into());
        assert_eq!(key.0.to_vec(), id.as_bytes().iter().copied().chain([44, 1, 0, 0].iter().copied()).collect_vec());
    }
}
