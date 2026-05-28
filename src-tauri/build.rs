fn main() {
  println!("cargo:rerun-if-changed=../public/icon.icns");
  println!("cargo:rerun-if-changed=../public/icon.png");
  println!("cargo:rerun-if-changed=../public/icon_64.png");
  tauri_build::build()
}
