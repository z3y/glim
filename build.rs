use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Shader {
    src: String,
    dst: String,
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shader_dir = Path::new("shaders");

    println!("cargo:rerun-if-changed=shaders");

    let mut shaders = Vec::new();

    shaders.push(Shader {
        src: "initialize_preview.slang".into(),
        dst: "initialize_preview.spv".into(),
    });

    shaders.push(Shader {
        src: "preview.slang".into(),
        dst: "preview.spv".into(),
    });

    shaders.push(Shader {
        src: "bake_direct_emission.slang".into(),
        dst: "bake_direct_emission.spv".into(),
    });

    shaders.push(Shader {
        src: "bake_direct_light.slang".into(),
        dst: "bake_direct_light.spv".into(),
    });

    shaders.push(Shader {
        src: "bake_indirect.slang".into(),
        dst: "bake_indirect.spv".into(),
    });

    shaders.push(Shader {
        src: "dilate.slang".into(),
        dst: "dilate.spv".into(),
    });

    shaders.push(Shader {
        src: "bake_sh.slang".into(),
        dst: "bake_sh.spv".into(),
    });

    shaders.push(Shader {
        src: "bake_shadowmask.slang".into(),
        dst: "bake_shadowmask.spv".into(),
    });

    for shader in shaders {
        let shader_path = shader_dir.join(shader.src);
        let spv_path = out_dir.join(shader.dst);

        let mut args = Vec::new();

        args.push("-stage");
        args.push("compute");
        args.push("-entry");
        args.push("main");

        let status = Command::new("slangc")
            .arg(shader_path.to_str().unwrap())
            .arg("-warnings-as-errors")
            .arg("all")
            .arg("-O3")
            .arg("-g0")
            .arg("-o")
            .arg(spv_path.to_str().unwrap())
            .args(["-target", "spirv"])
            .args(args)
            .status()
            .expect("Failed to run slangc");

        if !status.success() {
            panic!("Slang compilation failed for {:?}", shader_path);
        }

        println!("spv_path: {:?}", spv_path);
    }
}
