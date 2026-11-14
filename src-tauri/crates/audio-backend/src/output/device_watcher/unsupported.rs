use super::{PendingChanges, PlatformBackend};

pub(super) struct Backend {
    events: PendingChanges,
}

impl PlatformBackend for Backend {
    fn start() -> Option<Self> {
        None
    }
    fn events(&self) -> &PendingChanges {
        &self.events
    }
}
