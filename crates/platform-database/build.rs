fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
    println!("cargo:rerun-if-changed=tests/compatibility");
}
