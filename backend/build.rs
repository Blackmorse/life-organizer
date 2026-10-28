fn main() {
    // SQLx embeds migrations at compile time; rebuild when SQL files change.
    println!("cargo:rerun-if-changed=migrations");
}
