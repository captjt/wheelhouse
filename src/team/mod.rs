//! Teams: a workspace joins each engineer's own Wheelhouse into one, so a
//! team works on shared dockets and its agents work together across
//! machines, on one signed record. docs/teams.md is the design.
//!
//! - `roster`: the member log and who's in the workspace.
//! - `shared`: who may write what on a shared docket, and what a member's
//!   hub keeps back.
//! - `auth`: requests a member's hub signs for the workspace hub.

pub mod auth;
pub mod roster;
pub mod shared;
