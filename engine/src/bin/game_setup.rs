//! First run and updates: the player's ROM read into the content tree, the asset index built and the translation
//! applied, all under the per-user data folder:
//!
//!   <data>/content/       the game's files (lp_engine::extract)
//!   <data>/patch/         the translation overlay (lp_engine::patchfile from translation-en.lpdelta)
//!   <data>/saves/         the battery save (loveplus.sav)
//!   <data>/recordings/    session records (game.rs Recorder)
//!   <data>/settings.json
//!
//! Dialogs are the system's own (rfd); everything here runs before the window opens.

use std::path::{Path, PathBuf};

/// the per-user data folder: macOS Library/Application Support, Windows %APPDATA%, else ~/.config
pub fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    Some(if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/LovePlus+")
    } else if cfg!(target_os = "windows") {
        env("APPDATA")?.join("LovePlus+")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?.join("loveplus")
    })
}

pub fn error(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(title).set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
}

fn ask(title: &str, text: &str) -> bool {
    rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok
}

/// where the translation file ships: beside the program, in a macOS bundle's Resources, in the data folder, or (when
/// run from the source tree) dist/
pub fn translation_file(data: &Path) -> Option<PathBuf> {
    let name = "translation-en.lpdelta";
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join(name), dir.join("../Resources").join(name), data.join(name), Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist").join(name)].into_iter().find(|p| p.is_file())
}

fn sha1_hex(b: &[u8]) -> String {
    use sha1::Digest;
    sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// a ROM chosen by the player, read into `content`; Err(message) when the player cancels or it fails
fn extract_from_rom(content: &Path, reason: &str) -> Result<(), String> {
    if !ask("LovePlus+", &format!("{reason}\n\nChoose your LovePlus+ game file (the .nds dump of your own cartridge). It is read once into the game's folder; nothing is changed in it.")) {
        return Err("no game file chosen".into());
    }
    loop {
        let Some(rom_path) = rfd::FileDialog::new().set_title("Choose your LovePlus+ game file").add_filter("Nintendo DS ROM", &["nds", "NDS"]).pick_file() else {
            return Err("no game file chosen".into());
        };
        let rom = std::fs::read(&rom_path).map_err(|e| format!("{}: {e}", rom_path.display()))?;
        match lp_engine::extract::identify(&rom) {
            Ok(_) => {}
            Err(e) => {
                error("Not LovePlus+", &format!("{e}.\n\nChoose the file of LovePlus+ (the Japanese release, game code BOXJ)."));
                continue;
            }
        }
        eprintln!("reading {} into {}", rom_path.display(), content.display());
        let mut last = String::new();
        lp_engine::extract::extract(&rom, content, &mut |stage, _, _| {
            if stage != last {
                eprintln!("  {stage}");
                last = stage.to_string();
            }
        })
        .map_err(|e| e.to_string())?;
        // the asset index (texture and screen names, for mods and dumps)
        let (textures, _) = lp_engine::assetindex::textures(content);
        let dir = content.join("index");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("textures.json"), lp_engine::assetindex::to_json(&textures)).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("planes.json"), lp_engine::assetindex::planes_to_json(&lp_engine::assetindex::planes(content))).map_err(|e| e.to_string())?;
        return Ok(());
    }
}

/// the content tree ready to run under `data` (read from the player's ROM on first run or after an extractor update)
/// and the translation applied (when its file is found and changed); the content folder
pub fn prepare(data: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let content = data.join("content");
    match lp_engine::extract::installed(&content) {
        Some((v, _)) if v >= lp_engine::extract::VERSION && lp_engine::extract::usable(&content) => {}
        Some(_) => extract_from_rom(&content, "This version of LovePlus+ reads the game's files differently, so they need to be read again.")?,
        None => extract_from_rom(&content, "Welcome! LovePlus+ needs the game's files before it can start.")?,
    }
    apply_translation(data, &content)?;
    Ok(content)
}

/// the translation overlay rebuilt when its file is newer than the one applied (patch/.source holds its sha1)
pub fn apply_translation(data: &Path, content: &Path) -> Result<(), String> {
    let patch = data.join("patch");
    let Some(file) = translation_file(data) else {
        eprintln!("no translation file found: the game runs in Japanese");
        return Ok(());
    };
    let delta = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let stamp = sha1_hex(&delta);
    if std::fs::read_to_string(patch.join(".source")).ok().as_deref() == Some(stamp.as_str()) && patch.join("fs").is_dir() {
        return Ok(());
    }
    eprintln!("applying the translation {}", file.display());
    lp_engine::patchfile::apply(content, &delta, &patch, &mut |_, _, _| {}).map_err(|e| format!("The translation could not be applied: {e}"))?;
    std::fs::write(patch.join(".source"), stamp).map_err(|e| e.to_string())?;
    Ok(())
}

/// the save file: imported from a file the player picks (the current one kept as loveplus.sav.bak); true when replaced
pub fn import_save(save: &Path) -> bool {
    let Some(src) = rfd::FileDialog::new().set_title("Import a LovePlus+ save").add_filter("DS save", &["sav", "SAV", "dsv", "bin"]).pick_file() else { return false };
    let Ok(b) = std::fs::read(&src) else {
        error("Import", "That file could not be read.");
        return false;
    };
    // DeSmuME's .dsv carries a 122-byte footer after the save itself
    let b = if src.extension().map_or(false, |e| e.eq_ignore_ascii_case("dsv")) && b.len() > 122 && b.ends_with(b"|-DESMUME SAVE-|") { b[..b.len() - 122].to_vec() } else { b };
    if b.len() < 0x1000 {
        error("Import", "That file is too small to be a LovePlus+ save.");
        return false;
    }
    if !ask("Import save", "Replace the current save with this one? The current save is kept as loveplus.sav.bak, and the game restarts.") {
        return false;
    }
    if save.exists() {
        let _ = std::fs::copy(save, save.with_extension("sav.bak"));
    }
    if let Some(d) = save.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    match std::fs::write(save, &b) {
        Ok(()) => true,
        Err(e) => {
            error("Import", &format!("The save could not be written: {e}"));
            false
        }
    }
}

/// the save file copied where the player chooses
pub fn export_save(save: &Path) {
    if !save.exists() {
        error("Export", "There is no save yet.");
        return;
    }
    if let Some(dst) = rfd::FileDialog::new().set_title("Export the LovePlus+ save").set_file_name("loveplus.sav").save_file() {
        if let Err(e) = std::fs::copy(save, &dst) {
            error("Export", &format!("The save could not be written: {e}"));
        }
    }
}

/// the program started again with the same arguments (after an import or a setting that applies at start)
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).args(std::env::args().skip(1)).spawn();
    }
}
