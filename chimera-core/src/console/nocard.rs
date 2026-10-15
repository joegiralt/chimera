//! `NoCard`: the card type of a unit whose console has none.

use chimera_hal::store::{
    ByteSink, Dir, FileName, FileRuns, Files, ReadSink, RunsError, Store, StoreError, VolumeId,
};

/// A unit with no card behind the console: `ls` and `put` are not in its
/// build.
#[derive(Debug)]
pub enum NoCard {}

impl Store for NoCard {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        match *self {}
    }
    fn list(
        &mut self,
        _: VolumeId,
        _: Dir,
        _: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        match *self {}
    }
    fn read(&mut self, _: VolumeId, _: FileName, _: &mut dyn ReadSink) -> Result<(), StoreError> {
        match *self {}
    }
    fn runs(&mut self, _: VolumeId, _: FileName) -> Result<FileRuns, RunsError> {
        match *self {}
    }
    fn write(
        &mut self,
        _: VolumeId,
        _: FileName,
        _: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        match *self {}
    }
    fn delete(&mut self, _: VolumeId, _: FileName) -> Result<(), StoreError> {
        match *self {}
    }
    fn make_dir(&mut self, _: VolumeId, _: Dir) -> Result<(), StoreError> {
        match *self {}
    }
}

impl Files for NoCard {
    fn cap(&mut self, _: Option<u32>) {
        match *self {}
    }
}
