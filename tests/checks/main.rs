//! Foundation checks that pass in ticket 1: dependency direction, the
//! fixture scan, the approved limits, schema conformance and the pending list.
//! OpenWiz 0.0.5 (the implementation) adds the skills folder's static check, and
//! the implementation the check that no test leaves a directory's mode to the umask.

#[path = "../support/schema.rs"]
mod schema;

#[allow(dead_code)]
#[path = "../support/dirs.rs"]
mod dirs;

mod dependencies;
mod dir_modes;
mod fixture_scan;
mod limits;
mod pending;
mod schema_conformance;
mod skills;
