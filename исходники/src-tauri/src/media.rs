//! Плеер для визуалов: что сейчас играет на компьютере. Linux: playerctl или D-Bus (MPRIS), Windows: системные медиа-сессии через PowerShell.
use crate::util::*;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static CACHE: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

pub fn now_playing() -> Value {
    {
        let c = CACHE.lock().unwrap();
        if let Some((t, v)) = c.as_ref() {
            if t.elapsed() < Duration::from_millis(1800) {
                return v.clone();
            }
        }
    }
    let v = read().unwrap_or_else(|| json!({"title": "", "artist": "", "pos": 0, "len": 0, "playing": false}));
    *CACHE.lock().unwrap() = Some((Instant::now(), v.clone()));
    v
}

fn run(prog: &str, args: &[&str]) -> Option<String> {
    let o = cmd(prog).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).to_string())
}

#[cfg(not(windows))]
fn read() -> Option<Value> {
    if let Some(s) = run("playerctl", &["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}\t{{mpris:length}}\t{{position}}"]) {
        let p: Vec<&str> = s.trim_end().split('\t').collect();
        if p.len() >= 5 && !p[1].is_empty() {
            let us = |x: &str| x.parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
            return Some(json!({"title": p[1], "artist": p[2], "len": us(p[3]), "pos": us(p[4]), "playing": p[0] == "Playing"}));
        }
    }
    // без playerctl: спрашиваем D-Bus напрямую
    let names = run("busctl", &["--user", "list", "--no-legend"])?;
    let name = names.lines().filter_map(|l| l.split_whitespace().next()).find(|n| n.starts_with("org.mpris.MediaPlayer2."))?.to_string();
    let prop = |p: &str| run("busctl", &["--user", "get-property", &name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player", p]);
    let meta = prop("Metadata")?;
    let after = |key: &str| -> Option<String> {
        let i = meta.find(&format!("\"{key}\""))? + key.len() + 2;
        Some(meta[i..].to_string())
    };
    let first_str = |s: &str| -> String {
        let a = s.find('"').map(|i| i + 1).unwrap_or(0);
        let rest = &s[a..];
        rest.find('"').map(|e| rest[..e].to_string()).unwrap_or_default()
    };
    let title = after("xesam:title").map(|s| first_str(&s)).unwrap_or_default();
    let artist = after("xesam:artist").map(|s| first_str(&s)).unwrap_or_default();
    let len = after("mpris:length").and_then(|s| s.split_whitespace().nth(1).and_then(|x| x.parse::<f64>().ok())).unwrap_or(0.0) / 1_000_000.0;
    let pos = prop("Position").and_then(|s| s.split_whitespace().nth(1).and_then(|x| x.parse::<f64>().ok())).unwrap_or(0.0) / 1_000_000.0;
    let playing = prop("PlaybackStatus").map(|s| s.contains("Playing")).unwrap_or(false);
    if title.is_empty() {
        return None;
    }
    Some(json!({"title": title, "artist": artist, "len": len, "pos": pos, "playing": playing}))
}

#[cfg(windows)]
fn read() -> Option<Value> {
    const PS: &str = r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | ? { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
Function Await($t, $r) { $m = $asTask.MakeGenericMethod($r); $n = $m.Invoke($null, @($t)); $n.Wait(-1) | Out-Null; $n.Result }
[Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager,Windows.Media.Control,ContentType=WindowsRuntime] | Out-Null
$mgr = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
$s = $mgr.GetCurrentSession()
if (-not $s) { exit 1 }
$p = Await ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
$t = $s.GetTimelineProperties(); $i = $s.GetPlaybackInfo()
@{title=$p.Title; artist=$p.Artist; pos=$t.Position.TotalSeconds; len=$t.EndTime.TotalSeconds; playing=($i.PlaybackStatus -eq 4)} | ConvertTo-Json -Compress
"#;
    let out = run("powershell", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", PS])?;
    let v: Value = serde_json::from_str(out.trim()).ok()?;
    if v["title"].as_str().unwrap_or("").is_empty() {
        return None;
    }
    Some(v)
}
