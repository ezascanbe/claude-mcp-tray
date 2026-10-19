//! Reading and toggling MCP servers in `claude_desktop_config.json`.
//!
//! Claude Desktop has no "disabled" flag for MCP servers, so disabling a server
//! moves its definition out of `mcpServers` into a sidecar store owned by this
//! app. Enabling moves it back. The rest of Claude's config is preserved as-is
//! (key order included).
//!
//! Desktop Extensions are different: each has its own settings file in
//! `Claude Extensions Settings/<id>.json` with an `isEnabled` flag, which we flip.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "claude_desktop_config.json";
const BACKUP_FILE: &str = "claude_desktop_config.mcp-tray-backup.json";

#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub enabled: bool,
    /// Short human-readable description: the command line or the remote URL.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub config_path: String,
    pub config_found: bool,
    pub servers: Vec<ServerInfo>,
    pub extensions: Vec<ExtensionInfo>,
}

const EXTENSIONS_FILE: &str = "extensions-installations.json";
const EXTENSION_SETTINGS_DIR: &str = "Claude Extensions Settings";

/// Claude's data directory (the folder holding `claude_desktop_config.json`).
fn claude_dir() -> PathBuf {
    config_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

fn extension_settings_path(id: &str) -> Result<PathBuf> {
    // Ids are reverse-DNS style; refuse anything that could escape the folder.
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err(anyhow!("invalid extension id \"{id}\""));
    }
    Ok(claude_dir().join(EXTENSION_SETTINGS_DIR).join(format!("{id}.json")))
}

fn list_extensions() -> Result<Vec<ExtensionInfo>> {
    let installs = read_json_object(&claude_dir().join(EXTENSIONS_FILE))?;
    let Some(Value::Object(exts)) = installs.get("extensions") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (id, ext) in exts {
        let manifest = ext.get("manifest");
        let text = |key: &str| manifest.and_then(|m| m.get(key)).and_then(Value::as_str);
        let name = text("display_name").or_else(|| text("name")).unwrap_or(id).to_string();
        let mut detail = String::from("Extension");
        if let Some(v) = ext.get("version").and_then(Value::as_str) {
            detail.push_str(&format!(" · v{v}"));
        }
        if let Some(author) = manifest.and_then(|m| m.get("author")).and_then(|a| a.get("name")).and_then(Value::as_str) {
            detail.push_str(&format!(" · {author}"));
        }
        // Claude treats a freshly installed extension (no settings file yet) as enabled.
        let enabled = read_json_object(&extension_settings_path(id)?)?
            .get("isEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        out.push(ExtensionInfo { id: id.clone(), name, enabled, detail });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

pub fn set_extension_enabled(id: &str, enabled: bool) -> Result<()> {
    let path = extension_settings_path(id)?;
    let mut settings = read_json_object(&path)?;
    settings.insert("isEnabled".into(), Value::Bool(enabled));
    write_json_atomic(&path, &settings)
}

/// Locates Claude Desktop's config file for this platform.
pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CLAUDE_MCP_TRAY_CONFIG") {
        return PathBuf::from(p);
    }
    candidates()
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| candidates().into_iter().next().expect("at least one candidate"))
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        out.push(home.join("Library/Application Support/Claude").join(CONFIG_FILE));
    }
    #[cfg(windows)]
    {
        if let Some(roaming) = dirs::config_dir() {
            out.push(roaming.join("Claude").join(CONFIG_FILE));
        }
        // MSIX installs may virtualise AppData into the package's LocalCache.
        if let Some(local) = dirs::data_local_dir() {
            if let Ok(entries) = fs::read_dir(local.join("Packages")) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with("Claude_") {
                        out.push(e.path().join("LocalCache/Roaming/Claude").join(CONFIG_FILE));
                    }
                }
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(cfg) = dirs::config_dir() {
        out.push(cfg.join("Claude").join(CONFIG_FILE));
    }
    out
}

/// Claude Desktop's log folder (holds `mcp.log` and `mcp-server-<name>.log`).
pub fn logs_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    candidates.extend(dirs::home_dir().map(|h| h.join("Library/Logs/Claude")));
    #[cfg(windows)]
    {
        // Current builds log under Local; older ones used Roaming.
        candidates.extend(dirs::data_local_dir().map(|d| d.join("Claude").join("Logs")));
        candidates.extend(dirs::config_dir().map(|d| d.join("Claude").join("logs")));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    candidates.extend(dirs::config_dir().map(|d| d.join("Claude").join("logs")));
    candidates.into_iter().find(|p| p.is_dir())
}

const STORE_FILE: &str = "claude-mcp-tray.disabled-servers.json";

/// Where definitions of disabled servers are kept: next to Claude's config.
///
/// Claude's own folder is deliberately used rather than a folder of our own,
/// because the Windows Store build of Claude virtualises other AppData folders
/// for any process started from inside it (e.g. a Claude Code terminal); a
/// store there would be invisible to the same app launched from Explorer.
fn store_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_MCP_TRAY_STORE") {
        return Ok(PathBuf::from(p));
    }
    Ok(claude_dir().join(STORE_FILE))
}

/// Stores written by earlier versions: `<config dir>/claude-mcp-tray/disabled_servers.json`,
/// plus any copy that MSIX virtualisation redirected into Claude's package cache.
fn legacy_store_paths() -> Vec<PathBuf> {
    if std::env::var_os("CLAUDE_MCP_TRAY_STORE").is_some() {
        return Vec::new();
    }
    let rel = Path::new("claude-mcp-tray").join("disabled_servers.json");
    let mut out: Vec<PathBuf> = dirs::config_dir().map(|d| d.join(&rel)).into_iter().collect();
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        if let Ok(entries) = fs::read_dir(local.join("Packages")) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") {
                    out.push(e.path().join("LocalCache").join("Roaming").join(&rel));
                }
            }
        }
    }
    out
}

/// Folds legacy stores into the current one (skipping servers that are active
/// again), then renames each legacy file so it is only imported once.
fn migrate_legacy_stores() -> Result<()> {
    let legacy: Vec<PathBuf> = legacy_store_paths().into_iter().filter(|p| p.is_file()).collect();
    if legacy.is_empty() {
        return Ok(());
    }
    let store_file = store_path()?;
    let mut store = read_json_object(&store_file)?;
    let config = read_json_object(&config_path())?;
    let active = config.get("mcpServers").and_then(Value::as_object);
    for path in &legacy {
        for (name, def) in read_json_object(path)? {
            let is_active = active.is_some_and(|a| a.contains_key(&name));
            if !is_active && !store.contains_key(&name) {
                store.insert(name, def);
            }
        }
    }
    write_json_atomic(&store_file, &store)?;
    for path in &legacy {
        let _ = fs::rename(path, path.with_extension("json.migrated"));
    }
    Ok(())
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?
    {
        Value::Object(m) => Ok(m),
        _ => Err(anyhow!("{} is not a JSON object", path.display())),
    }
}

/// Writes JSON via a temp file + rename so a crash never leaves a half-written file.
fn write_json_atomic(path: &Path, value: &Map<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    // Match the existing file's trailing newline so diffs stay minimal.
    let had_newline = fs::read(path).map(|b| b.ends_with(b"\n")).unwrap_or(true);
    if had_newline {
        text.push('\n');
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Keeps a copy of the config as it was before this app first touched it.
fn ensure_backup(config: &Path) -> Result<()> {
    let backup = config.with_file_name(BACKUP_FILE);
    if config.exists() && !backup.exists() {
        fs::copy(config, &backup).with_context(|| format!("backing up to {}", backup.display()))?;
    }
    Ok(())
}

fn describe(def: &Value) -> String {
    if let Some(url) = def.get("url").and_then(Value::as_str) {
        return url.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(cmd) = def.get("command").and_then(Value::as_str) {
        let base = Path::new(cmd)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| cmd.to_string());
        parts.push(base);
    }
    if let Some(args) = def.get("args").and_then(Value::as_array) {
        parts.extend(args.iter().filter_map(Value::as_str).map(str::to_string));
    }
    parts.join(" ")
}

pub fn list() -> Result<Listing> {
    migrate_legacy_stores()?;
    let path = config_path();
    let config = read_json_object(&path)?;
    let store = read_json_object(&store_path()?)?;

    let mut servers: Vec<ServerInfo> = Vec::new();
    if let Some(Value::Object(active)) = config.get("mcpServers") {
        for (name, def) in active {
            servers.push(ServerInfo { name: name.clone(), enabled: true, detail: describe(def) });
        }
    }
    for (name, def) in &store {
        if !servers.iter().any(|s| &s.name == name) {
            servers.push(ServerInfo { name: name.clone(), enabled: false, detail: describe(def) });
        }
    }
    servers.sort_by_key(|s| s.name.to_lowercase());

    Ok(Listing {
        config_path: path.display().to_string(),
        config_found: path.exists(),
        servers,
        extensions: list_extensions()?,
    })
}

pub fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    migrate_legacy_stores()?;
    let path = config_path();
    let store_file = store_path()?;
    let mut config = read_json_object(&path)?;
    let mut store = read_json_object(&store_file)?;

    let servers = config
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("\"mcpServers\" in {} is not an object", path.display()))?;

    if enabled {
        if servers.contains_key(name) {
            // Already active (e.g. re-added by hand); drop any stale stash.
            if store.remove(name).is_some() {
                write_json_atomic(&store_file, &store)?;
            }
            return Ok(());
        }
        let def = store
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("no saved definition for \"{name}\""))?;
        servers.insert(name.to_string(), def);
        ensure_backup(&path)?;
        write_json_atomic(&path, &config)?;
        store.remove(name);
        write_json_atomic(&store_file, &store)?;
    } else {
        let Some(def) = servers.shift_remove(name) else {
            return Ok(()); // already disabled
        };
        // Stash first so the definition is never lost if the second write fails.
        store.insert(name.to_string(), def);
        write_json_atomic(&store_file, &store)?;
        ensure_backup(&path)?;
        write_json_atomic(&path, &config)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests redirect paths through process-wide env vars, so they must not overlap.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn disable_then_enable_round_trips() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("mcp-tray-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join(CONFIG_FILE);
        let original = "{\n  \"mcpServers\": {\n    \"b\": {\n      \"command\": \"npx\",\n      \"args\": [\n        \"-y\",\n        \"srv\"\n      ]\n    },\n    \"a\": {\n      \"url\": \"https://x/mcp\"\n    }\n  },\n  \"preferences\": {\n    \"z\": true,\n    \"y\": 1\n  }\n}\n";
        fs::write(&cfg, original).unwrap();
        std::env::set_var("CLAUDE_MCP_TRAY_CONFIG", &cfg);
        std::env::set_var("CLAUDE_MCP_TRAY_STORE", dir.join("store.json"));

        set_enabled("b", false).unwrap();
        let l = list().unwrap();
        assert_eq!(l.servers.iter().map(|s| (s.name.as_str(), s.enabled)).collect::<Vec<_>>(), [("a", true), ("b", false)]);
        assert_eq!(l.servers[1].detail, "npx -y srv");
        assert!(!fs::read_to_string(&cfg).unwrap().contains("\"b\""));
        assert!(dir.join(BACKUP_FILE).exists());

        set_enabled("b", true).unwrap();
        assert!(list().unwrap().servers.iter().all(|s| s.enabled));
        // Order of other keys is preserved; the re-enabled server moves to the end.
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
        assert!(text.find("\"z\"").unwrap() < text.find("\"y\"").unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extension_toggle_keeps_user_config() {
        let dir = std::env::temp_dir().join(format!("mcp-tray-ext-{}", std::process::id()));
        let settings = dir.join(EXTENSION_SETTINGS_DIR);
        fs::create_dir_all(&settings).unwrap();
        fs::write(
            dir.join(EXTENSIONS_FILE),
            r#"{"extensions":{"x.fs":{"version":"1.2.0","manifest":{"display_name":"Filesystem","author":{"name":"Anthropic"}}},"x.new":{"manifest":{"name":"New"}}}}"#,
        )
        .unwrap();
        fs::write(settings.join("x.fs.json"), r#"{"isEnabled":false,"userConfig":{"dirs":["D:\\a"]}}"#).unwrap();
        let ids_only = |l: Vec<ExtensionInfo>| l.into_iter().map(|e| (e.name, e.enabled)).collect::<Vec<_>>();

        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CLAUDE_MCP_TRAY_CONFIG", dir.join(CONFIG_FILE));
        let exts = list_extensions().unwrap();
        assert_eq!(exts[0].detail, "Extension · v1.2.0 · Anthropic");
        assert_eq!(ids_only(exts), [("Filesystem".to_string(), false), ("New".to_string(), true)]);

        set_extension_enabled("x.fs", true).unwrap();
        let text = fs::read_to_string(settings.join("x.fs.json")).unwrap();
        assert!(text.contains("\"isEnabled\": true") && text.contains("D:\\\\a"));
        assert!(set_extension_enabled("../evil", true).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
