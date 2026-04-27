use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(PartialEq)]
enum ShaderType {
    Compute,
    Vertex,
    Fragment,
    Geometry,
}

struct Shader {
    ty: ShaderType,
    src: String,
    dst: String,
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shader_dir = Path::new("shaders");

    let slangc = env::var("SLANG_DIR")
        .map(|p| PathBuf::from(p).join("bin/slangc"))
        .or_else(|_| env::var("VULKAN_SDK").map(|p| PathBuf::from(p).join("bin/slangc")))
        .unwrap_or_else(|_| PathBuf::from("slangc"));

    println!("cargo:rerun-if-changed=shaders");

    let mut shaders = Vec::new();

    shaders.push(Shader {
        ty: ShaderType::Compute,
        src: "test.slang".into(),
        dst: "test.spv".into(),
    });

    shaders.push(Shader {
        ty: ShaderType::Compute,
        src: "init_from_camera.slang".into(),
        dst: "init_from_camera.spv".into(),
    });

    shaders.push(Shader {
        ty: ShaderType::Compute,
        src: "bake.slang".into(),
        dst: "bake.spv".into(),
    });

    shaders.push(Shader {
        ty: ShaderType::Vertex,
        src: "init_from_bake.slang".into(),
        dst: "init_from_bake_vertex.spv".into(),
    });

    shaders.push(Shader {
        ty: ShaderType::Geometry,
        src: "init_from_bake.slang".into(),
        dst: "init_from_bake_geometry.spv".into(),
    });

    shaders.push(Shader {
        ty: ShaderType::Fragment,
        src: "init_from_bake.slang".into(),
        dst: "init_from_bake_fragment.spv".into(),
    });

    for shader in shaders {
        let shader_path = shader_dir.join(shader.src);
        let spv_path = out_dir.join(shader.dst);

        let mut args = Vec::new();

        if shader.ty == ShaderType::Compute {
            args.push("-stage");
            args.push("compute");

            args.push("-entry");
            args.push("main");
        }

        if shader.ty == ShaderType::Fragment {
            args.push("-stage");
            args.push("fragment");

            args.push("-entry");
            args.push("fragment");
        }

        if shader.ty == ShaderType::Vertex {
            args.push("-stage");
            args.push("vertex");

            args.push("-entry");
            args.push("vertex");
        }

        if shader.ty == ShaderType::Geometry {
            args.push("-stage");
            args.push("geometry");

            args.push("-entry");
            args.push("geometry");
        }

        let status = Command::new(&slangc)
            .arg(shader_path.to_str().unwrap())
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
