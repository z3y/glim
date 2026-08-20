fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("wrapper.cpp")
        .compile("tinybvh_wrapper");

    println!("cargo:rerun-if-changed=wrapper.cpp");
    println!("cargo:rerun-if-changed=tiny_bvh.h");
}
