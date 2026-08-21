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

const TOKEN: &str = "secret";

struct Daemon {
    dir: std::path::PathBuf,
    child: std::process::Child,
    port: u16,
    _stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl Daemon {
    fn start(dir: &std::path::Path) -> Self {
        Self::start_with_docker(dir, true)
    }

    /// `containers_alive` : ce que répond le docker factice à `docker inspect`.
    fn start_with_docker(dir: &std::path::Path, containers_alive: bool) -> Self {
        use std::io::BufRead;
        use std::os::unix::fs::PermissionsExt;

        std::fs::create_dir_all(dir).unwrap();
        let docker = dir.join("fake-docker.sh");
        let script = format!("#!/bin/sh\n[ \"$1\" = inspect ] && echo {containers_alive}\nexit 0\n");
        std::fs::write(&docker, script).unwrap();
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_kgo"))
            .args(["node", "start", "--port", "0", "--data-dir", dir.to_str().unwrap()])
            .env("KGO_TOKEN", TOKEN)
            .env("KGO_DOCKER", &docker) // docker factice : toutes les commandes réussissent
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut port = None;
        let mut line = String::new();
        while port.is_none() && stdout.read_line(&mut line).unwrap() > 0 {
            port = line.trim().strip_prefix("tcp : 0.0.0.0:").map(|p| p.parse().unwrap());
            line.clear();
        }
        let port = port.expect("le daemon n'a pas annoncé son port");
        Self { dir: dir.to_path_buf(), child, port, _stdout: stdout }
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

/// Attend que `kgo ps` affiche `expected` (le démarrage est asynchrone).
fn wait_for_ps(daemon: &Daemon, expected: &str) -> String {
    for _ in 0..100 {
        let ps = text(&daemon.kgo(&["ps"]).stdout);
        if ps.contains(expected) {
            return ps;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("`kgo ps` n'a jamais affiché « {expected} »");
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

    let ps = wait_for_ps(&daemon, "running");
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

/// Envoie une requête JSON brute sur le port TCP du nœud, précédée du token.
fn tcp_request(port: u16, token: &str, request: &str) -> String {
    use std::io::{BufRead, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    writeln!(stream, "{token}\n{request}").unwrap();
    let mut line = String::new();
    std::io::BufReader::new(stream).read_line(&mut line).unwrap();
    line
}

#[test]
fn le_port_tcp_exige_le_token_hors_info() {
    let dir = std::env::temp_dir().join(format!("kgo-tcp-{}", std::process::id()));
    let daemon = Daemon::start(&dir);

    let info = tcp_request(daemon.port, "", r#""Info""#);
    assert!(info.contains("\"cpus\"") && info.contains(&format!("\"port\":{}", daemon.port)), "{info}");

    let ps = r#"{"Ps":{"all":true}}"#;
    for token in ["", "mauvais"] {
        let refused = tcp_request(daemon.port, token, ps);
        assert!(refused.contains("refusée"), "{refused}");
    }
    assert!(tcp_request(daemon.port, TOKEN, ps).contains("Workloads"));

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn join_et_nodes() {
    let base = std::env::temp_dir().join(format!("kgo-nodes-{}", std::process::id()));
    let (dir_a, dir_b) = (base.join("a"), base.join("b"));
    let a = Daemon::start(&dir_a);
    let b = Daemon::start(&dir_b);

    let addr_b = format!("127.0.0.1:{}", b.port);
    let out = a.kgo(&["node", "join", &addr_b]);
    assert!(out.status.success(), "{}", text(&out.stderr));

    let listing = text(&a.kgo(&["nodes"]).stdout);
    assert!(listing.contains(&addr_b) && listing.contains("up"), "{listing}");
    assert_eq!(listing.matches(" up").count(), 2, "{listing}");

    // un pair injoignable est signalé mais n'empêche pas la liste
    b.stop();
    let listing = text(&a.kgo(&["nodes"]).stdout);
    assert!(listing.contains("injoignable"), "{listing}");
    assert!(listing.contains("localhost"), "{listing}");

    // on ne retient pas une adresse qui ne répond pas
    let out = a.kgo(&["node", "join", &addr_b]);
    assert_eq!(out.status.code(), Some(1));

    a.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn reconciliation_au_redemarrage() {
    let dir = std::env::temp_dir().join(format!("kgo-reconcile-{}", std::process::id()));
    let daemon = Daemon::start(&dir);
    let out = daemon.kgo(&["run", "nginx"]);
    let id = text(&out.stdout).split_whitespace().next().unwrap().to_string();
    wait_for_ps(&daemon, "running");

    // le daemon redémarre alors que le conteneur a disparu : le workload devient failed
    daemon.stop();
    let daemon = Daemon::start_with_docker(&dir, false);
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains("aucun workload"));
    let all = text(&daemon.kgo(&["ps", "--all"]).stdout);
    assert!(all.contains(&id) && all.contains("failed"), "{all}");

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_ps_stop_sur_un_pair() {
    let base = std::env::temp_dir().join(format!("kgo-forward-{}", std::process::id()));
    let (a, b) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")));
    let on = format!("127.0.0.1:{}", b.port);

    let out = a.kgo(&["--on", &on, "run", "nginx"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let id = text(&out.stdout).split_whitespace().next().unwrap().to_string();

    // le workload vit sur b, pas sur a
    assert!(text(&a.kgo(&["ps"]).stdout).contains("aucun workload"));
    assert!(text(&b.kgo(&["ps"]).stdout).contains(&id));
    assert!(text(&a.kgo(&["--on", &on, "ps"]).stdout).contains(&id));

    let out = a.kgo(&["--on", &on, "stop", &id]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&b.kgo(&["ps"]).stdout).contains("aucun workload"));

    a.stop();
    b.stop();
    std::fs::remove_dir_all(&base).ok();
}
