fn main() {
    // La página del jugador va dentro de la app (`red::pagina`, con rust-embed). Se construye con
    // `bun run web:build:app`; si aún no está (p. ej. `cargo test` en limpio), se crea la carpeta vacía para que
    // compile: el servidor dirá entonces que falta la página.
    let _ = std::fs::create_dir_all("../build-jugador");
    tauri_build::build()
}
