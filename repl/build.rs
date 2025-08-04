fn main() {
    println!("cargo:rustc-link-search=native=./lib");
    println!("cargo:rustc-link-lib=dylib=sqlite3");
    println!("cargo:rerun-if-changed=build.rs");
}
