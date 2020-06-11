#![allow(non_snake_case, non_upper_case_globals)]
#![deny(clippy::option_unwrap_used)]

mod types;
use crate::types::*;
mod utils;
use crate::utils::*;
mod language_client;
mod language_server_protocol;
mod logger;
mod rpcclient;
mod rpchandler;
mod sign;
mod viewport;
mod vim;
mod vimext;

use failure::{bail, err_msg, format_err, Error, Fail, ResultExt};
use jsonrpc_core::{self as rpc, Params, Value};
use log::{debug, error, info, log_enabled, warn};
use lsp_types::{self as lsp, *};
use maplit::hashmap;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::prelude::*;
use std::io::{BufRead, BufReader, BufWriter};
use std::net::TcpStream;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use url::Url;

#[derive(Debug, StructOpt)]
struct Arguments {}

fn main() -> Fallible<()> {
    let version = format!("{} {}", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
    let args = Arguments::clap().version(version.as_str());
    let _ = args.get_matches();

    let (tx, rx) = crossbeam::channel::unbounded();
    let language_client = language_client::LanguageClient {
        version: Arc::new(version),
        state_mutex: Arc::new(Mutex::new(State::new(tx)?)),
        clients_mutex: Arc::new(Mutex::new(HashMap::new())),
    };

    language_client.loop_call(&rx)
}
