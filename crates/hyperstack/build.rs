//! Windows resources: the application icon, version info and a manifest
//! declaring `asInvoker` and per-monitor-v2 DPI awareness (docs/03 §13).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/hyperstack.ico");
    #[cfg(windows)]
    {
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/hyperstack.ico");
        res.set("FileDescription", "HyperStack");
        res.set("ProductName", "HyperStack");
        res.set("FileVersion", version);
        res.set("ProductVersion", version);
        res.set("OriginalFilename", "HyperStack.exe");
        res.set("LegalCopyright", "Copyright © 2026 owner@zacx.net");
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=Windows resources were not compiled: {e}");
        }
    }
}
