fn main() {
    // The built interface is embedded in the executable: rebuild whenever `npm run build`
    // changes it (cargo scans the whole folder for modifications).
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build();
}
