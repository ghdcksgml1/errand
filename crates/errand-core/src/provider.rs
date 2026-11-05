//! Errand follows exactly one Codex account and one Claude account: the ones signed in on this computer.

/// The two services Errand watches. There is no slot for a third account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Provider {
    Codex,
    Claude,
}

impl Provider {
    /// Display order: the dock, menus and details list Codex first.
    pub const ALL: [Provider; 2] = [Provider::Codex, Provider::Claude];

    pub fn title(self) -> &'static str {
        match self {
            Provider::Codex => "Codex",
            Provider::Claude => "Claude",
        }
    }

    /// The key Errand's saved records use. It dates from when Codex was account A and Claude account C.
    pub fn storage_key(self) -> &'static str {
        match self {
            Provider::Codex => "a",
            Provider::Claude => "c",
        }
    }

    pub fn from_storage_key(key: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|provider| provider.storage_key() == key)
    }

    /// Prefix of the task-card keys built from this service's sessions.
    pub fn session_key_prefix(self) -> &'static str {
        match self {
            Provider::Codex => "codex:",
            Provider::Claude => "claude:",
        }
    }
}

/// One value for each service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerProvider<T> {
    pub codex: T,
    pub claude: T,
}

impl<T> PerProvider<T> {
    pub fn from_fn(mut value: impl FnMut(Provider) -> T) -> PerProvider<T> {
        PerProvider { codex: value(Provider::Codex), claude: value(Provider::Claude) }
    }

    /// In display order.
    pub fn iter(&self) -> impl Iterator<Item = (Provider, &T)> {
        [(Provider::Codex, &self.codex), (Provider::Claude, &self.claude)].into_iter()
    }
}

impl<T> std::ops::Index<Provider> for PerProvider<T> {
    type Output = T;
    fn index(&self, provider: Provider) -> &T {
        match provider {
            Provider::Codex => &self.codex,
            Provider::Claude => &self.claude,
        }
    }
}

impl<T> std::ops::IndexMut<Provider> for PerProvider<T> {
    fn index_mut(&mut self, provider: Provider) -> &mut T {
        match provider {
            Provider::Codex => &mut self.codex,
            Provider::Claude => &mut self.claude,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_keys_round_trip_and_reject_the_retired_account() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_storage_key(provider.storage_key()), Some(provider));
        }
        assert_eq!(Provider::from_storage_key("b"), None);
    }
}
