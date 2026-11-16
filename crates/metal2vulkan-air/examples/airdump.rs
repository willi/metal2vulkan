//! Prints the module of a metallib's function (diagnostics): airdump LIB.metallib NAME.
use metal2vulkan_air::budget::{Budget, Meter};
use metal2vulkan_air::container::{parse, Library};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).unwrap();
    let Library::Metallib(lib) = parse(&bytes).unwrap() else {
        panic!("not a metallib")
    };
    for f in &lib.functions {
        if f.name != args[2] {
            continue;
        }
        let mut m = Meter::new(&Budget::standard());
        let d = metal2vulkan_air::decode(f.bitcode, &mut m).unwrap();
        println!("{}", metal2vulkan_air::ir::print::print(&d.module));
    }
}
