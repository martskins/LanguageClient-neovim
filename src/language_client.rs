use crate::{
    config::Config,
    types::{LanguageId, State},
    utils::diff_value,
    vim::Vim,
};
use anyhow::Result;
use log::*;
use serde_json::Value;

use parking_lot::{Mutex, MutexGuard, RwLock};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

pub struct SafeLock<T> {
    inner: Arc<RwLock<T>>,
}

impl<T> Clone for SafeLock<T> {
    fn clone(&self) -> Self {
        SafeLock {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> SafeLock<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub fn get<K>(&self, f: impl FnOnce(&T) -> K) -> K {
        f(self.inner.read().deref())
    }

    pub fn update<K>(&self, f: impl FnOnce(&mut T) -> K) -> K {
        let mut state = self.inner.write();
        let mut state = state.deref_mut();
        f(&mut state)
    }
}

#[derive(Clone)]
pub struct LanguageClient {
    version: String,
    state_mutex: Arc<Mutex<State>>,
    clients_mutex: Arc<Mutex<HashMap<LanguageId, Arc<Mutex<()>>>>>,
    pub config: SafeLock<Config>,
}

impl LanguageClient {
    pub fn new(version: String, state: State) -> Self {
        LanguageClient {
            version,
            state_mutex: Arc::new(Mutex::new(state)),
            clients_mutex: Arc::new(Mutex::new(HashMap::new())),
            config: SafeLock::new(Config::default()),
        }
    }

    pub fn version(&self) -> String {
        self.version.clone()
    }

    // NOTE: Don't expose this as public.
    // MutexGuard could easily halt the program when one guard is not released immediately after use.
    fn lock(&self) -> MutexGuard<State> {
        self.state_mutex.lock()
    }

    // This fetches a mutex that is unique to the provided languageId.
    //
    // Here, we return a mutex instead of the mutex guard because we need to satisfy the borrow
    // checker. Otherwise, there is no way to guarantee that the mutex in the hash map wouldn't be
    // garbage collected as a result of another modification updating the hash map, while something was holding the lock
    pub fn get_client_update_mutex(&self, language_id: LanguageId) -> Result<Arc<Mutex<()>>> {
        let mut map = self.clients_mutex.lock();
        if !map.contains_key(&language_id) {
            map.insert(language_id.clone(), Arc::new(Mutex::new(())));
        }
        let mutex: Arc<Mutex<()>> = map.get(&language_id).unwrap().clone();
        Ok(mutex)
    }

    pub fn get<T>(&self, f: impl FnOnce(&State) -> T) -> Result<T> {
        Ok(f(self.lock().deref()))
    }

    pub fn update<T>(&self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let mut state = self.lock();
        let mut state = state.deref_mut();

        let v = if log_enabled!(log::Level::Debug) {
            let s = serde_json::to_string(&state)?;
            serde_json::from_str(&s)?
        } else {
            Value::default()
        };

        let result = f(&mut state);

        let next_v = if log_enabled!(log::Level::Debug) {
            let s = serde_json::to_string(&state)?;
            serde_json::from_str(&s)?
        } else {
            Value::default()
        };

        for (k, (v1, v2)) in diff_value(&v, &next_v, "state") {
            debug!("{}: {} ==> {}", k, v1, v2);
        }
        result
    }

    pub fn vim(&self) -> Result<Vim> {
        self.get(|state| state.vim.clone())
    }
}
