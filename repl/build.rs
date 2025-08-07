fn main() {
    println!("cargo:rustc-link-search=native=./lib");

    #[cfg(feature = "static")]
    {
        println!("cargo:rustc-link-lib=static=sqlite3");
        // For static linking, we also need to link the system libraries that SQLite depends on
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=m");
    }

    #[cfg(not(feature = "static"))]
    {
        println!("cargo:rustc-link-lib=dylib=sqlite3");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=./lib/libsqlite3.a");
}
