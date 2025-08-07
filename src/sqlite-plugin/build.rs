extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=sqlite3/sqlite3.h");
    println!("cargo:rerun-if-changed=sqlite3/sqlite3ext.h");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be defined"));
    let vars_path = out_path.join("vars.rs");
    let bindings_path = out_path.join("bindings.rs");

    // Optimization: Skip bindgen if files already exist and headers haven't changed
    if vars_path.exists() && bindings_path.exists() {
        return;
    }

    // Optimization: Configure bindgen for faster generation
    let vars = bindgen::Builder::default()
        .header("sqlite3/sqlite3ext.h")
        .allowlist_item("SQLITE_.*")
        .use_core()
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .size_t_is_usize(true)
        .generate_comments(false) // Skip comments for faster generation
        .generate()
        .expect("Unable to generate bindings");

    let bindings = bindgen::Builder::default()
        .header("sqlite3/sqlite3ext.h")
        .blocklist_item("SQLITE_.*")
        .use_core()
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .size_t_is_usize(true)
        .generate_comments(false) // Skip comments for faster generation
        .generate()
        .expect("Unable to generate bindings");

    vars.write_to_file(vars_path).expect("Couldn't write vars!");
    bindings
        .write_to_file(bindings_path)
        .expect("Couldn't write bindings!");
}
