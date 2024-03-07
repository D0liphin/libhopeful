// use std::{env, ffi::OsString, path::Path, process::Command};

// const CC: &str = "gcc";

// struct Build {
//     out_dir: OsString,
// }

// impl Build {
//     fn new() -> Self {
//         Self {
//             out_dir: env::var_os("OUT_DIR").unwrap(),
//         }
//     }

//     /// Make an object file, `src` is relative to `src/` and `dst` is relative to
//     /// `$OUT_DIR/`.
//     fn makeo(self, src: &str, dst: &str) -> Self {
//         let src = Path::new("src").join(src);
//         Command::new("gcc")
//             .arg(&src)
//             .arg("-c")
//             .arg("-o")
//             .arg(Path::new(&self.out_dir).join(dst))
//             .status()
//             .unwrap();
//         println!("cargo:rerun-if-changed={}", src.to_string_lossy());
//         self
//     }

//     /// The lib is called `lib{lib}.a`
//     fn makelib(self, o: &str, lib: &str) -> Self {
//         Command::new("ar")
//             .arg("crus")
//             .arg(&format!("lib{lib}.a"))
//             .arg(o)
//             .current_dir(Path::new(&self.out_dir))
//             .status()
//             .unwrap();
//         println!("cargo:rustc-link-lib=static={lib}");
//         self
//     }

//     fn finish(self) {
//         println!(
//             "cargo:rustc-link-search=native={}",
//             Path::new(&self.out_dir).display()
//         );
//     }
// }

fn main() {
    cc::Build::new()
        .file("src/clib/dlmalloc.c")
        .compile("dlmalloc");
    println!("cargo:rerun-if-changed=src/clib/dlmalloc.c");
}
