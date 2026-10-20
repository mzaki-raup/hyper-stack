//! Write the two templates as SVG, PNG and PDF for a visual check:
//! `cargo run -p hs-diagram --example export -- <out-dir>`.

use hs_diagram::export::{PngOptions, pdf, png, svg};
use hs_diagram::{layout_approx, template};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::temp_dir()
            .join("hs-diagram-out")
            .display()
            .to_string()
    });
    std::fs::create_dir_all(&out)?;
    for (name, doc) in [
        ("flow", template::flow()),
        ("architecture", template::architecture()),
    ] {
        let scene = layout_approx(&doc);
        let base = std::path::Path::new(&out).join(name);
        std::fs::write(base.with_extension("svg"), svg(&scene, Some("#ffffff")))?;
        std::fs::write(
            base.with_extension("png"),
            png(
                &scene,
                &PngOptions {
                    scale: 2.0,
                    background: Some("#ffffff".into()),
                },
            )?,
        )?;
        std::fs::write(base.with_extension("pdf"), pdf(&scene, Some("#ffffff"))?)?;
        println!("{name}: {}×{}", scene.size.w, scene.size.h);
    }
    println!("written to {out}");
    Ok(())
}
