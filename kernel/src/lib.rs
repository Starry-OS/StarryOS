//! The core functionality of a monolithic kernel, including loading user
//! programs and managing processes.

#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![feature(concat_bytes)]
#![feature(c_variadic)]
#![feature(layout_for_ptr)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;
extern crate axruntime;

#[macro_use]
extern crate axlog;

pub mod entry;

mod config;
pub mod file;
pub mod mm;
mod pseudofs;
pub mod syscall;
mod task;
mod time;

pub mod bpf;
mod exception;
pub mod kmod;
pub mod kprobe;
pub mod lock_api;
pub mod perf;
pub mod tracepoint;
pub mod uprobe;