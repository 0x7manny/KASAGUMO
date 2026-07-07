use std::process::{Command, Output};

fn kgo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kgo"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_liste_les_commandes() {
    let out = kgo(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for cmd in ["node", "nodes", "run", "ps", "stop", "chunk"] {
        assert!(stdout.contains(cmd), "{cmd} absent de l'aide");
    }
}

#[test]
fn run_sans_daemon_explique_quoi_faire() {
    let dir = std::env::temp_dir().join(format!("kgo-norun-{}", std::process::id()));
    let out = kgo(&["run", "--data-dir", dir.to_str().unwrap(), "nginx"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("kgo node start"));
}

#[test]
fn commande_non_implementee_echoue() {
    let out = kgo(&["nodes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("pas encore implémenté"));
}

#[test]
fn memoire_invalide_est_refusee() {
    let out = kgo(&["run", "--memory", "4TB", "nginx"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unité inconnue"));
}

#[test]
fn chunk_fichier_inexistant() {
    let out = kgo(&["chunk", "/nope/introuvable.png"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("impossible de découper"));
}

#[test]
fn chunk_sur_un_vrai_fichier() {
    let path = std::env::temp_dir().join(format!("kgo-test-{}.bin", std::process::id()));
    std::fs::write(&path, vec![7u8; 3 * 1024 * 1024]).unwrap();
    let out = kgo(&["chunk", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("intégrité : OK"));
}

#[test]
fn ps_sans_daemon_explique_quoi_faire() {
    let dir = std::env::temp_dir().join(format!("kgo-nodaemon-{}", std::process::id()));
    let out = kgo(&["ps", "--data-dir", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("kgo node start"));
}

struct Daemon {
    dir: std::path::PathBuf,
    child: std::process::Child,
}

impl Daemon {
    fn start(dir: &std::path::Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_kgo"))
            .args(["node", "start", "--data-dir", dir.to_str().unwrap()])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let socket = dir.join("kgo.sock");
        let daemon = Self { dir: dir.to_path_buf(), child };
        for _ in 0..100 {
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                return daemon;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("le daemon n'a pas démarré");
    }

    fn kgo(&self, args: &[&str]) -> Output {
        let mut full = args.to_vec();
        full.extend(["--data-dir", self.dir.to_str().unwrap()]);
        kgo(&full)
    }

    fn stop(self) {}
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn ps_parle_au_daemon() {
    let dir = std::env::temp_dir().join(format!("kgo-daemon-{}", std::process::id()));
    let daemon = Daemon::start(&dir);
    let out = daemon.kgo(&["ps"]);
    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();

    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("aucun workload"));
}

#[test]
fn cycle_de_vie_et_persistance() {
    let dir = std::env::temp_dir().join(format!("kgo-cycle-{}", std::process::id()));
    let daemon = Daemon::start(&dir);

    let out = daemon.kgo(&["run", "--cpu", "2", "--memory", "4GB", "nginx:latest"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("pending"));
    let id = stdout.split_whitespace().next().unwrap().to_string();

    let ps = text(&daemon.kgo(&["ps"]).stdout);
    assert!(ps.contains(&id) && ps.contains("nginx:latest") && ps.contains("4096MB"));

    // le daemon est tué brutalement puis relancé : l'état doit survivre
    daemon.stop();
    let daemon = Daemon::start(&dir);
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains(&id));

    let out = daemon.kgo(&["stop", &id]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains("aucun workload"));
    assert!(text(&daemon.kgo(&["ps", "--all"]).stdout).contains("stopped"));

    let out = daemon.kgo(&["stop", "inconnu"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("introuvable"));

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}
