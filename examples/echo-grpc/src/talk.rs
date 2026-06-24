//! echo-grpc / talk — the CLIENT that proves the echo roundtrip over the private gRPC.
//!
//! It connects to a running `listen` server's private gRPC address, then:
//!   1. POKE: builds the cause `[%echo <value>]`, jams it, and sends it as a gRPC poke.
//!      The server's kernel +poke stores the value and acks.
//!   2. PEEK: builds the path `/echo`, jams it, and sends it as a gRPC peek. The server's
//!      kernel +peek returns `[~ ~ <value>]`; the private gRPC server jams that whole result
//!      and ships it back. We cue it and pull out the echoed value, printing it.
//!
//! Usage:
//!   talk [--grpc-addr http://127.0.0.1:5561] [--value <text>]
//!
//! Defaults: addr http://127.0.0.1:5561, value "hello world". Exits non-zero if the peeked
//! value does not match what we poked (so it doubles as a roundtrip self-test).

use std::error::Error;

use nockapp::noun::slab::NounSlab;
use nockapp::utils::make_tas;
use nockapp::{AtomExt, Bytes};
use nockapp_grpc::services::private_nockapp::PrivateNockAppGrpcClient;
use nockapp_grpc::wire_conversion::create_grpc_wire;
use noun_serde::NounDecode;
use nockvm::noun::{NounAllocator, D, T};
use nockvm_macros::tas;
use tracing::info;

const DEFAULT_ADDR: &str = "http://127.0.0.1:5561";
const DEFAULT_VALUE: &str = "hello world";
const PID: i32 = 42;

fn parse_args() -> (String, String) {
    let mut addr = DEFAULT_ADDR.to_string();
    let mut value = DEFAULT_VALUE.to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--grpc-addr" => {
                if let Some(v) = args.next() {
                    addr = v;
                }
            }
            "--value" => {
                if let Some(v) = args.next() {
                    value = v;
                }
            }
            other => {
                if let Some(v) = other.strip_prefix("--grpc-addr=") {
                    addr = v.to_string();
                } else if let Some(v) = other.strip_prefix("--value=") {
                    value = v.to_string();
                }
            }
        }
    }
    (addr, value)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = nockapp::kernel::boot::default_boot_cli(false);
    nockapp::kernel::boot::init_default_tracing(&cli);

    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        // already installed; fine.
    }

    let (addr, value) = parse_args();
    info!("echo-grpc talk: connecting to {addr}");

    let mut client = PrivateNockAppGrpcClient::connect(addr.clone()).await?;

    // ---- POKE: cause [%echo <value>] -------------------------------------------------
    // The poke payload IS the cause noun the kernel's +poke receives.
    let mut poke_slab: NounSlab = NounSlab::new();
    let tag = D(tas!(b"echo"));
    let val_atom = nockvm::noun::Atom::from_bytes(&mut poke_slab, &Bytes::from(value.as_bytes().to_vec()));
    let cause = T(&mut poke_slab, &[tag, val_atom.as_noun()]);
    poke_slab.set_root(cause);
    let poke_payload = poke_slab.jam().to_vec();

    let wire = create_grpc_wire();
    let acked = client.poke(PID, wire, poke_payload).await?;
    println!("POKE  [%echo {value:?}] -> acked={acked}");
    if !acked {
        return Err("poke was not acknowledged".into());
    }

    // ---- PEEK: path /echo ------------------------------------------------------------
    // Build the path noun ~[%echo] = [%echo ~] and jam it; the server cues it and feeds it
    // to the kernel's +peek as a `path`.
    let mut path_slab: NounSlab = NounSlab::new();
    let echo_knot = make_tas(&mut path_slab, "echo").as_noun();
    let path = T(&mut path_slab, &[echo_knot, D(0)]);
    path_slab.set_root(path);
    let path_jam = path_slab.jam().to_vec();

    let result_jam = client.peek(PID, path_jam).await?;

    // Cue the peek result. It is the full kernel +peek output `[~ ~ val]` == [0 0 val].
    let mut res_slab: NounSlab = NounSlab::new();
    let res_noun = res_slab.cue_into(Bytes::from(result_jam))?;
    let space = res_slab.noun_space();

    // Walk [0 [0 val]] to extract `val`. (The full kernel +peek result is `[~ ~ val]`.)
    let outer = res_noun
        .in_space(&space)
        .as_cell()
        .map_err(|_| "peek result is not a (unit ...)")?;
    let inner = outer
        .tail()
        .as_cell()
        .map_err(|_| "peek result inner unit is empty (~): no value stored yet")?;
    let val_noun = inner.tail().noun();
    // Decode the value atom as a UTF-8 cord via noun-serde's String decoder.
    let echoed = String::from_noun(&val_noun, &space)
        .map_err(|e| format!("could not decode peeked value as string: {e}"))?;

    println!("PEEK  /echo -> {echoed:?}");

    if echoed == value {
        println!("ROUNDTRIP OK: poked {value:?} == peeked {echoed:?}");
        Ok(())
    } else {
        Err(format!("ROUNDTRIP MISMATCH: poked {value:?} != peeked {echoed:?}").into())
    }
}
