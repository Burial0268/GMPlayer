use std::path::Path;

use crate::error::AudioResult;
use crate::metadata;
use crate::types::AudioInfo;

pub fn extract_metadata_only(path: &Path) -> AudioResult<AudioInfo> {
    metadata::extract_metadata_only(path)
}
